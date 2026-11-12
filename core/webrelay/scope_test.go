package webrelay

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/coder/websocket"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// scopeUpstream is a real offline Bedrock server on loopback that reports each player's departure.
type scopeUpstream struct {
	address string
	joined  chan *minecraft.Conn
	left    chan *minecraft.Conn
}

// newScopeUpstream accepts players, sends them a spawn startup and watches them until they leave.
func newScopeUpstream(t *testing.T) *scopeUpstream {
	t.Helper()
	listener, err := minecraft.ListenConfig{AuthenticationDisabled: true, ErrorLog: slog.New(slog.DiscardHandler)}.Listen("raknet", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	upstream := &scopeUpstream{address: listener.Addr().String(), joined: make(chan *minecraft.Conn, 8), left: make(chan *minecraft.Conn, 8)}
	go func() {
		for {
			accepted, err := listener.Accept()
			if err != nil {
				return
			}
			server := accepted.(*minecraft.Conn)
			if server.WritePacketImmediate(scopeStartup()...) != nil {
				_ = server.Close()
				continue
			}
			upstream.joined <- server
			go func() {
				for {
					if _, err := server.ReadPacket(); err != nil {
						upstream.left <- server
						return
					}
				}
			}()
		}
	}()
	return upstream
}

// scopeStartup is the minimal startup a relay join hands off to the browser.
func scopeStartup() []packet.Packet {
	return []packet.Packet{
		&packet.StartGame{WorldName: "Scope", EntityUniqueID: 7, EntityRuntimeID: 7,
			BaseGameVersion: bridgecontract.GameVersion, GameVersion: bridgecontract.GameVersion},
		&packet.ItemRegistry{Items: []protocol.ItemEntry{{Name: "minecraft:shield", RuntimeID: 355}}},
		&packet.CreativeContent{},
		&packet.ChunkRadiusUpdated{ChunkRadius: 4},
		&packet.PlayStatus{Status: packet.PlayStatusPlayerSpawn},
	}
}

// requireJoin waits for the upstream to accept the next player.
func (upstream *scopeUpstream) requireJoin(t *testing.T) *minecraft.Conn {
	t.Helper()
	select {
	case server := <-upstream.joined:
		return server
	case <-time.After(testwait.DefaultTimeout):
		t.Fatal("the upstream server did not accept the relay join")
		return nil
	}
}

// requireLeft waits until server's player has left the upstream server.
func (upstream *scopeUpstream) requireLeft(t *testing.T, server *minecraft.Conn) {
	t.Helper()
	timeout := time.After(testwait.DefaultTimeout)
	for {
		select {
		case departed := <-upstream.left:
			if departed == server {
				return
			}
		case <-timeout:
			t.Fatal("the player stayed connected to the upstream server")
		}
	}
}

// scopeHost serves the relay handler the way an embedding server does, behind its own middleware.
type scopeHost struct {
	url string
	mu  sync.Mutex
	// requests holds the cancellation of every request context the middleware created.
	requests []context.CancelFunc
}

// newScopeHost serves handler from loopback; every request gets a context the host can cancel.
func newScopeHost(t *testing.T, handler http.Handler) *scopeHost {
	t.Helper()
	host := new(scopeHost)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		ctx, cancel := context.WithCancel(r.Context())
		defer cancel()
		host.mu.Lock()
		host.requests = append(host.requests, cancel)
		host.mu.Unlock()
		handler.ServeHTTP(w, r.WithContext(ctx))
	}))
	t.Cleanup(server.Close)
	host.url = "ws" + strings.TrimPrefix(server.URL, "http")
	return host
}

// cancelRequests cancels every request context the host's middleware handed to the relay.
func (host *scopeHost) cancelRequests() {
	host.mu.Lock()
	defer host.mu.Unlock()
	for _, cancel := range host.requests {
		cancel()
	}
}

// dial opens one relay channel with the browser's cookie header; a refused upgrade returns its status.
func (host *scopeHost) dial(t *testing.T, channel, cookie string) (net.Conn, int) {
	t.Helper()
	path := bridgecontract.RelayControlPath
	if channel == bridgecontract.RelayChannelPlay {
		path = bridgecontract.RelayPlayPath
	}
	header := http.Header{}
	if cookie != "" {
		header.Set("Cookie", cookie)
	}
	ctx, cancel := context.WithTimeout(t.Context(), testwait.DefaultTimeout)
	defer cancel()
	socket, response, err := websocket.Dial(ctx, host.url+path, &websocket.DialOptions{
		Subprotocols: []string{bridgecontract.RelaySocketProtocol}, HTTPHeader: header})
	if err != nil {
		if response == nil {
			t.Fatal(err)
		}
		return nil, response.StatusCode
	}
	t.Cleanup(func() { _ = socket.CloseNow() })
	// The connection lives as long as the test; only the relay or the test closes it.
	return websocket.NetConn(t.Context(), socket, websocket.MessageBinary), http.StatusSwitchingProtocols
}

// scopeControl sends a control Hello and returns its first event, followed by the account event on success.
func scopeControl(t *testing.T, conn net.Conn, token string) controlEvent {
	t.Helper()
	if err := writeJSON(t.Context(), conn, hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelControl,
		Channels: []string{bridgecontract.RelayChannelControl, bridgecontract.RelayChannelPlay}, Resume: token}); err != nil {
		t.Fatal(err)
	}
	var event controlEvent
	if err := readJSON(conn, &event); err != nil {
		t.Fatal(err)
	}
	if event.Type == bridgecontract.RelayEventWelcome {
		var account controlEvent
		if err := readJSON(conn, &account); err != nil || account.Account == nil {
			t.Fatal("missing account response", err)
		}
	}
	return event
}

// scopeConnect is an offline Connect to target whose client data passes the core's validation.
func scopeConnect(t *testing.T, target, name string) []byte {
	t.Helper()
	clientData, err := json.Marshal(login.ClientData{
		DeviceOS: 7, GameVersion: minecraft.DefaultProtocol.Ver(), LanguageCode: "en_US", SkinID: "skin",
		SkinData:       base64.StdEncoding.EncodeToString(make([]byte, 64*32*4)),
		SkinImageWidth: 64, SkinImageHeight: 32, ThirdPartyName: name,
	})
	if err != nil {
		t.Fatal(err)
	}
	request, err := json.Marshal(map[string]any{
		"protocol":     minecraft.DefaultProtocol.ID(),
		"target":       map[string]string{"kind": control.TargetRakNet, "value": target},
		"client_cache": false,
		"client_data":  json.RawMessage(clientData),
	})
	if err != nil {
		t.Fatal(err)
	}
	return append([]byte{bridgecontract.SessionKindConnect}, request...)
}

// scopePlay opens play for welcome's generation and joins the upstream until the core hands off.
func scopePlay(t *testing.T, conn net.Conn, welcome controlEvent, upstream *scopeUpstream) *streamnet.FramedConn {
	t.Helper()
	if err := writeJSON(t.Context(), conn, hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelPlay,
		Resume: welcome.Resume, Generation: welcome.Generation}); err != nil {
		t.Fatal(err)
	}
	framed := streamnet.NewFramedConn(conn)
	if _, err := framed.Write(scopeConnect(t, upstream.address, "Scope")); err != nil {
		t.Fatal(err)
	}
	for {
		frame, err := framed.ReadPacket()
		if err != nil {
			t.Fatal("play ended before the handoff", err)
		}
		switch frame[0] {
		case bridgecontract.SessionKindHandoff:
			return framed
		case bridgecontract.SessionKindDisconnect, bridgecontract.SessionKindTransfer:
			t.Fatalf("the core refused the join: %s", frame[1:])
		}
	}
}

// requireBatch waits for the next gameplay batch on play.
func requireBatch(t *testing.T, play *streamnet.FramedConn) {
	t.Helper()
	for {
		frame, err := play.ReadPacket()
		if err != nil {
			t.Fatal("play ended before the upstream batch arrived", err)
		}
		if frame[0] == bridgecontract.SessionKindBatch {
			return
		}
	}
}

// requireControlAnswer sends an account command and waits for its outcome, skipping progress events.
// A signed-out session that is playing answers sign-out with a busy refusal.
func requireControlAnswer(t *testing.T, conn net.Conn) {
	t.Helper()
	if err := writeJSON(t.Context(), conn, controlRequest{Op: bridgecontract.RelayOpSignOut}); err != nil {
		t.Fatal("control refused a command", err)
	}
	for {
		var event controlEvent
		if err := readJSON(conn, &event); err != nil {
			t.Fatal("control ended before answering", err)
		}
		if event.Type == bridgecontract.RelayEventAccount || event.Type == bridgecontract.RelayEventError {
			return
		}
	}
}

// scopeRelay returns a library relay whose sessions may join a loopback upstream.
func scopeRelay(t *testing.T, cfg Config) *Relay {
	t.Helper()
	cfg.Logger = slog.New(slog.DiscardHandler)
	relay := New(cfg)
	relay.dialer = newUpstreamDialer(true)
	t.Cleanup(relay.Close)
	return relay
}

// The relay owns its sessions: an embedding server's request context does not end a live game.
// Middleware that only cancels the request therefore cannot revoke play; scopes do that.
func TestRequestContextCancellationDoesNotEndRelayGame(t *testing.T) {
	relay := scopeRelay(t, Config{})
	upstream := newScopeUpstream(t)
	host := newScopeHost(t, relay.Handler())
	controlConn, _ := host.dial(t, bridgecontract.RelayChannelControl, "")
	welcome := scopeControl(t, controlConn, "")
	playConn, _ := host.dial(t, bridgecontract.RelayChannelPlay, "")
	play := scopePlay(t, playConn, welcome, upstream)
	server := upstream.requireJoin(t)

	host.cancelRequests()

	for range 2 {
		requireControlAnswer(t, controlConn)
		if err := server.WritePacket(&packet.SetTime{Time: 1}); err != nil {
			t.Fatal(err)
		}
		if err := server.Flush(); err != nil {
			t.Fatal("the upstream player left after the request context ended", err)
		}
		requireBatch(t, play)
	}
	select {
	case <-upstream.left:
		t.Fatal("the upstream player left after the request context ended")
	default:
	}
}

// errNoAdmission is the embedding server's refusal of a request without a valid access cookie.
var errNoAdmission = errors.New("no admission")

// scopeAdmissions stands in for the embedding server's access store: an access cookie names one
// admission, which the server can revoke while browsers still play under it.
type scopeAdmissions struct {
	mu      sync.Mutex
	entries map[string]*scopeAdmission
}

// scopeAdmission is one granted access with its own revocation.
type scopeAdmission struct {
	deadline time.Time
	ctx      context.Context
	cancel   context.CancelFunc
}

// newScopeAdmissions returns an empty access store whose admissions end with the test.
func newScopeAdmissions(t *testing.T) *scopeAdmissions {
	t.Helper()
	admissions := &scopeAdmissions{entries: make(map[string]*scopeAdmission)}
	t.Cleanup(func() {
		admissions.mu.Lock()
		defer admissions.mu.Unlock()
		for _, entry := range admissions.entries {
			entry.cancel()
		}
	})
	return admissions
}

// grant admits id until deadline and returns its browser cookie header.
func (admissions *scopeAdmissions) grant(id string, deadline time.Time) string {
	ctx, cancel := context.WithCancel(context.Background())
	admissions.mu.Lock()
	admissions.entries[id] = &scopeAdmission{deadline: deadline, ctx: ctx, cancel: cancel}
	admissions.mu.Unlock()
	return "access=" + id
}

// revoke ends id's admission, as a logout or password change does.
func (admissions *scopeAdmissions) revoke(id string) {
	admissions.mu.Lock()
	defer admissions.mu.Unlock()
	admissions.entries[id].cancel()
}

// resolve maps a request's access cookie to its server-side scope.
func (admissions *scopeAdmissions) resolve(r *http.Request) (Scope, error) {
	cookie, err := r.Cookie("access")
	if err != nil {
		return Scope{}, errNoAdmission
	}
	admissions.mu.Lock()
	defer admissions.mu.Unlock()
	entry := admissions.entries[cookie.Value]
	if entry == nil {
		return Scope{}, errNoAdmission
	}
	return Scope{ID: cookie.Value, Deadline: entry.deadline, Revoked: entry.ctx}, nil
}

// requireClosed waits until the relay closes conn; nothing else in the test closes it.
func requireClosed(t *testing.T, conn net.Conn, what string) {
	t.Helper()
	closed := make(chan struct{})
	go func() {
		defer close(closed)
		_, _ = io.Copy(io.Discard, conn)
	}()
	select {
	case <-closed:
	case <-time.After(testwait.DefaultTimeout):
		t.Fatalf("%s stayed open", what)
	}
}

func TestScopeRevocationEndsBothChannelsAndUpstream(t *testing.T) {
	admissions := newScopeAdmissions(t)
	relay := scopeRelay(t, Config{ResolveScope: admissions.resolve})
	upstream := newScopeUpstream(t)
	host := newScopeHost(t, relay.Handler())
	cookie := admissions.grant("A", time.Now().Add(time.Hour))
	controlConn, _ := host.dial(t, bridgecontract.RelayChannelControl, cookie)
	welcome := scopeControl(t, controlConn, "")
	playConn, _ := host.dial(t, bridgecontract.RelayChannelPlay, cookie)
	scopePlay(t, playConn, welcome, upstream)
	server := upstream.requireJoin(t)

	admissions.revoke("A")

	requireClosed(t, controlConn, "control")
	requireClosed(t, playConn, "play")
	upstream.requireLeft(t, server)
}
