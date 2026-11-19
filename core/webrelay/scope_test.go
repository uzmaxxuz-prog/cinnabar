package webrelay

import (
	"context"
	"crypto/tls"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strings"
	"sync"
	"testing"
	"testing/synctest"
	"time"

	"github.com/coder/websocket"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"github.com/quic-go/webtransport-go"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"golang.org/x/time/rate"
)

// scopeUpstream is a real offline Bedrock server on loopback that reports each player's departure.
type scopeUpstream struct {
	address string
	joined  chan *minecraft.Conn
	left    chan *minecraft.Conn
}

// newScopeUpstream accepts players on loopback, sends them a spawn startup and watches them until they leave.
func newScopeUpstream(t *testing.T) *scopeUpstream {
	t.Helper()
	return newScopeUpstreamAt(t, "127.0.0.1:0")
}

// newScopeUpstreamAt serves the same upstream on address.
func newScopeUpstreamAt(t *testing.T, address string) *scopeUpstream {
	t.Helper()
	listener, err := minecraft.ListenConfig{AuthenticationDisabled: true, ErrorLog: slog.New(slog.DiscardHandler)}.Listen("raknet", address)
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

// scopeRelay returns a library relay whose only game server is upstream; nil configures none.
func scopeRelay(t *testing.T, cfg Config, upstream *scopeUpstream) *Relay {
	t.Helper()
	cfg.Logger = slog.New(slog.DiscardHandler)
	if upstream != nil {
		cfg.GameEndpoint = netip.MustParseAddrPort(upstream.address)
	}
	relay := New(cfg)
	t.Cleanup(relay.Close)
	return relay
}

// The relay owns its sessions: an embedding server's request context does not end a live game.
// Middleware that only cancels the request therefore cannot revoke play; scopes do that.
func TestRequestContextCancellationDoesNotEndRelayGame(t *testing.T) {
	upstream := newScopeUpstream(t)
	relay := scopeRelay(t, Config{}, upstream)
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
	upstream := newScopeUpstream(t)
	relay := scopeRelay(t, Config{ResolveScope: admissions.resolve}, upstream)
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

// loopback is the one peer address every browser in these tests shares, as behind one NAT.
var loopback = netip.MustParseAddr("127.0.0.1")

// unlimitedActions lifts the shared address's action budget, so a refusal under test can only
// come from the scope rules. Transport and account bounds still apply.
func unlimitedActions(relay *Relay) {
	relay.mu.Lock()
	defer relay.mu.Unlock()
	relay.limiters[loopback] = &clientLimiter{limiter: rate.NewLimiter(rate.Inf, 1), lastSeen: time.Now()}
}

// awaitTransports waits until at most open transports from the shared address remain counted.
func awaitTransports(t *testing.T, relay *Relay, open int) {
	t.Helper()
	testwait.Eventually(t, testwait.DefaultTimeout, "released transport slots", func() bool {
		relay.mu.Lock()
		defer relay.mu.Unlock()
		return relay.connections[loopback] <= open
	})
}

// requireRefusedPlay opens play with welcome's capability and requires the relay to close it
// without handing off a join.
func requireRefusedPlay(t *testing.T, conn net.Conn, welcome controlEvent, upstream *scopeUpstream) {
	t.Helper()
	if err := writeJSON(t.Context(), conn, hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelPlay,
		Resume: welcome.Resume, Generation: welcome.Generation}); err != nil {
		t.Fatal(err)
	}
	framed := streamnet.NewFramedConn(conn)
	if _, err := framed.Write(scopeConnect(t, upstream.address, "Intruder")); err != nil {
		return
	}
	for {
		frame, err := framed.ReadPacket()
		if err != nil {
			return
		}
		if frame[0] == bridgecontract.SessionKindHandoff {
			t.Fatal("play joined a session of another admission")
		}
	}
}

// requireAlive proves that a browser's control, play and upstream player all still work.
func requireAlive(t *testing.T, controlConn net.Conn, play *streamnet.FramedConn, server *minecraft.Conn) {
	t.Helper()
	requireControlAnswer(t, controlConn)
	if err := server.WritePacket(&packet.SetTime{Time: 1}); err != nil {
		t.Fatal(err)
	}
	if err := server.Flush(); err != nil {
		t.Fatal("the upstream player left", err)
	}
	requireBatch(t, play)
}

func TestScopeIsolatesBrowsersBehindOneAddress(t *testing.T) {
	admissions := newScopeAdmissions(t)
	upstream := newScopeUpstream(t)
	relay := scopeRelay(t, Config{ResolveScope: admissions.resolve}, upstream)
	unlimitedActions(relay)
	host := newScopeHost(t, relay.Handler())
	cookieA := admissions.grant("A", time.Now().Add(time.Hour))
	cookieB := admissions.grant("B", time.Now().Add(time.Hour))
	controlA, _ := host.dial(t, bridgecontract.RelayChannelControl, cookieA)
	welcomeA := scopeControl(t, controlA, "")
	controlB, _ := host.dial(t, bridgecontract.RelayChannelControl, cookieB)
	if welcomeB := scopeControl(t, controlB, ""); welcomeB.Type != bridgecontract.RelayEventWelcome || welcomeB.Resume == welcomeA.Resume {
		t.Fatal("a second admission behind the same address did not get its own session")
	}

	cross, _ := host.dial(t, bridgecontract.RelayChannelControl, cookieB)
	if refused := scopeControl(t, cross, welcomeA.Resume); refused.Type != bridgecontract.RelayEventError || refused.Error != bridgecontract.RelayRefusalResume {
		t.Fatalf("another admission's cookie resumed the session with its bearer: %+v", refused)
	}
	requireClosed(t, cross, "refused control")
	awaitTransports(t, relay, 2)
	// A's play slot is free, so only the scope rule can refuse this join.
	crossPlay, _ := host.dial(t, bridgecontract.RelayChannelPlay, cookieB)
	requireRefusedPlay(t, crossPlay, welcomeA, upstream)
	awaitTransports(t, relay, 2)
	playConnA, _ := host.dial(t, bridgecontract.RelayChannelPlay, cookieA)
	playA := scopePlay(t, playConnA, welcomeA, upstream)
	serverA := upstream.requireJoin(t)
	requireAlive(t, controlA, playA, serverA)

	resumed, _ := host.dial(t, bridgecontract.RelayChannelControl, cookieA)
	if welcome := scopeControl(t, resumed, welcomeA.Resume); welcome.Type != bridgecontract.RelayEventWelcome {
		t.Fatalf("the same bearer could not resume with its own admission: %+v", welcome)
	}
}

func TestScopeBearerWithoutAdmissionIsRefusedBeforeUpgrade(t *testing.T) {
	admissions := newScopeAdmissions(t)
	relay := scopeRelay(t, Config{ResolveScope: admissions.resolve}, nil)
	host := newScopeHost(t, relay.Handler())
	cookie := admissions.grant("A", time.Now().Add(time.Hour))
	expired := admissions.grant("expired", time.Now().Add(-time.Second))
	controlConn, _ := host.dial(t, bridgecontract.RelayChannelControl, cookie)
	welcome := scopeControl(t, controlConn, "")
	for _, test := range []struct{ name, cookie string }{
		{name: "no cookie"},
		{name: "unknown cookie", cookie: "access=unknown"},
		{name: "expired admission", cookie: expired},
	} {
		for _, channel := range []string{bridgecontract.RelayChannelControl, bridgecontract.RelayChannelPlay} {
			if conn, status := host.dial(t, channel, test.cookie); conn != nil || status != http.StatusForbidden {
				t.Fatalf("%s: %s upgraded with status %d", test.name, channel, status)
			}
		}
	}
	resumed, _ := host.dial(t, bridgecontract.RelayChannelControl, cookie)
	if event := scopeControl(t, resumed, welcome.Resume); event.Type != bridgecontract.RelayEventWelcome {
		t.Fatalf("the bearer could not resume with its admission: %+v", event)
	}
}

func TestScopeRevocationRefusesItsBearersAndRevivesNothing(t *testing.T) {
	admissions := newScopeAdmissions(t)
	upstream := newScopeUpstream(t)
	relay := scopeRelay(t, Config{ResolveScope: admissions.resolve}, upstream)
	unlimitedActions(relay)
	host := newScopeHost(t, relay.Handler())
	cookieA := admissions.grant("A", time.Now().Add(time.Hour))
	controlConn, _ := host.dial(t, bridgecontract.RelayChannelControl, cookieA)
	welcome := scopeControl(t, controlConn, "")
	playConn, _ := host.dial(t, bridgecontract.RelayChannelPlay, cookieA)
	scopePlay(t, playConn, welcome, upstream)
	server := upstream.requireJoin(t)

	admissions.revoke("A")
	requireClosed(t, controlConn, "control")
	requireClosed(t, playConn, "play")
	upstream.requireLeft(t, server)
	testwait.Eventually(t, testwait.DefaultTimeout, "revoked session state and quota", func() bool {
		relay.mu.Lock()
		defer relay.mu.Unlock()
		return len(relay.sessions) == 0 && len(relay.tokens) == 0 && len(relay.scopes) == 0 && relay.perIP[loopback] == 0
	})
	awaitTransports(t, relay, 0)

	if conn, status := host.dial(t, bridgecontract.RelayChannelControl, cookieA); conn != nil || status != http.StatusForbidden {
		t.Fatalf("a revoked admission upgraded control with status %d", status)
	}
	cookieB := admissions.grant("B", time.Now().Add(time.Hour))
	stale, _ := host.dial(t, bridgecontract.RelayChannelControl, cookieB)
	if refused := scopeControl(t, stale, welcome.Resume); refused.Type != bridgecontract.RelayEventError || refused.Error != bridgecontract.RelayRefusalResume {
		t.Fatalf("a new admission revived the revoked session with its bearer: %+v", refused)
	}
	fresh, _ := host.dial(t, bridgecontract.RelayChannelControl, cookieB)
	next := scopeControl(t, fresh, "")
	if next.Type != bridgecontract.RelayEventWelcome {
		t.Fatalf("a new admission could not start a fresh session: %+v", next)
	}
	nextPlay, _ := host.dial(t, bridgecontract.RelayChannelPlay, cookieB)
	scopePlay(t, nextPlay, next, upstream)
	upstream.requireJoin(t)
}

func TestFreshControlInScopeClosesPreviousSession(t *testing.T) {
	admissions := newScopeAdmissions(t)
	upstream := newScopeUpstream(t)
	relay := scopeRelay(t, Config{ResolveScope: admissions.resolve}, upstream)
	unlimitedActions(relay)
	host := newScopeHost(t, relay.Handler())
	cookie := admissions.grant("A", time.Now().Add(time.Hour))
	oldControl, _ := host.dial(t, bridgecontract.RelayChannelControl, cookie)
	oldWelcome := scopeControl(t, oldControl, "")
	oldPlay, _ := host.dial(t, bridgecontract.RelayChannelPlay, cookie)
	scopePlay(t, oldPlay, oldWelcome, upstream)
	oldServer := upstream.requireJoin(t)

	newControl, _ := host.dial(t, bridgecontract.RelayChannelControl, cookie)
	if welcome := scopeControl(t, newControl, ""); welcome.Type != bridgecontract.RelayEventWelcome {
		t.Fatalf("fresh control was refused: %+v", welcome)
	}
	requireClosed(t, oldControl, "previous control")
	requireClosed(t, oldPlay, "previous play")
	upstream.requireLeft(t, oldServer)
	awaitTransports(t, relay, 1)
	stale, _ := host.dial(t, bridgecontract.RelayChannelControl, cookie)
	if refused := scopeControl(t, stale, oldWelcome.Resume); refused.Type != bridgecontract.RelayEventError || refused.Error != bridgecontract.RelayRefusalResume {
		t.Fatalf("the replaced session's bearer still resumed: %+v", refused)
	}
	requireControlAnswer(t, newControl)
	relay.mu.Lock()
	retained := relay.perIP[loopback]
	relay.mu.Unlock()
	if retained != 1 {
		t.Fatalf("one admission retained %d sessions", retained)
	}
}

func TestNetworkBreakResumesWithinScope(t *testing.T) {
	admissions := newScopeAdmissions(t)
	upstream := newScopeUpstream(t)
	relay := scopeRelay(t, Config{ResolveScope: admissions.resolve}, upstream)
	unlimitedActions(relay)
	host := newScopeHost(t, relay.Handler())
	cookie := admissions.grant("A", time.Now().Add(time.Hour))
	oldControl, _ := host.dial(t, bridgecontract.RelayChannelControl, cookie)
	welcome := scopeControl(t, oldControl, "")
	oldPlay, _ := host.dial(t, bridgecontract.RelayChannelPlay, cookie)
	scopePlay(t, oldPlay, welcome, upstream)
	oldServer := upstream.requireJoin(t)

	_ = oldControl.Close()
	_ = oldPlay.Close()
	upstream.requireLeft(t, oldServer)
	awaitTransports(t, relay, 0)

	controlConn, _ := host.dial(t, bridgecontract.RelayChannelControl, cookie)
	resumed := scopeControl(t, controlConn, welcome.Resume)
	if resumed.Type != bridgecontract.RelayEventWelcome || resumed.Generation <= welcome.Generation {
		t.Fatalf("a network break lost the session inside its valid admission: %+v", resumed)
	}
	playConn, _ := host.dial(t, bridgecontract.RelayChannelPlay, cookie)
	play := scopePlay(t, playConn, resumed, upstream)
	requireAlive(t, controlConn, play, upstream.requireJoin(t))
}

// testScope is an admission that the test revokes through cancel.
func testScope(t *testing.T, id string, deadline time.Time) (Scope, context.CancelFunc) {
	t.Helper()
	revoked, cancel := context.WithCancel(context.Background())
	t.Cleanup(cancel)
	return Scope{ID: id, Deadline: deadline, Revoked: revoked}, cancel
}

func TestScopeDeadlineEndsRetainedSessionAtItsFirstDeadline(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		relay := testRelay(t)
		client := netip.MustParseAddr("192.0.2.1")
		start := time.Now()
		first, _ := testScope(t, "A", start.Add(time.Hour))
		controller, detach, err := relay.openControl(first, client, "", start, testBrowserUserAgent, false)
		if err != nil {
			t.Fatal(err)
		}
		token := controller.session.token
		detach(start)
		later, _ := testScope(t, "A", start.Add(2*time.Hour))
		resumed, detach, err := relay.openControl(later, client, token, start, testBrowserUserAgent, false)
		if err != nil || resumed.session != controller.session {
			t.Fatal("a later request of the same admission could not resume", err)
		}
		defer func() { detach(time.Now()) }()
		welcome := testWelcome(t, relay, resumed, false, start)
		_, playCtx, err := relay.playSession(later, nil, hello{Resume: welcome.Resume, Generation: welcome.Generation}, start)
		if err != nil {
			t.Fatal(err)
		}

		<-time.After(time.Hour - time.Second)
		synctest.Wait()
		if resumed.ctx.Err() != nil {
			t.Fatal("the session ended before its deadline")
		}
		<-time.After(time.Second)
		synctest.Wait()
		if resumed.ctx.Err() == nil || playCtx.Err() == nil {
			t.Fatal("the admission deadline did not end control and play")
		}
		if _, _, err := relay.openControl(later, client, welcome.Resume, time.Now(), testBrowserUserAgent, false); !errors.Is(err, errResume) {
			t.Fatal("a later deadline of the same admission extended the session", err)
		}
		relay.mu.Lock()
		defer relay.mu.Unlock()
		if len(relay.sessions) != 0 || len(relay.tokens) != 0 || relay.perIP[client] != 0 {
			t.Fatal("the expired admission kept its session, bearers or quota")
		}
	})
}

func TestExpiredResumeNeedsFreshControlThatRetiresPrevious(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		relay := testRelay(t)
		client := netip.MustParseAddr("192.0.2.1")
		start := time.Now()
		scope, _ := testScope(t, "A", start.Add(time.Hour))
		previous, detach, err := relay.openControl(scope, client, "", start, testBrowserUserAgent, false)
		if err != nil {
			t.Fatal(err)
		}
		token := previous.session.token
		detach(start)
		<-time.After(resumeTTL)
		if _, _, err := relay.openControl(scope, client, token, time.Now(), testBrowserUserAgent, false); !errors.Is(err, errResume) {
			t.Fatal("an expired bearer resumed", err)
		}
		fresh, detach, err := relay.openControl(scope, client, "", time.Now(), testBrowserUserAgent, false)
		if err != nil || fresh.session == previous.session {
			t.Fatal("fresh control did not start a new session", err)
		}
		defer func() { detach(time.Now()) }()
		synctest.Wait()
		previous.session.mu.Lock()
		closed := previous.session.closed
		previous.session.mu.Unlock()
		relay.mu.Lock()
		_, kept := relay.tokens[token]
		sessions, retained := len(relay.sessions), relay.perIP[client]
		relay.mu.Unlock()
		if !closed || kept || sessions != 1 || retained != 1 {
			t.Fatal("fresh control left the previous retained session in place")
		}
		<-time.After(time.Hour - resumeTTL)
		synctest.Wait()
		if fresh.ctx.Err() == nil {
			t.Fatal("the fresh session outlived the admission deadline")
		}
	})
}

func TestScopeRefusesWebTransportWithoutAdmission(t *testing.T) {
	admissions := newScopeAdmissions(t)
	relay := scopeRelay(t, Config{ResolveScope: admissions.resolve}, nil)
	certificateServer := httptest.NewTLSServer(http.NotFoundHandler())
	certificate := certificateServer.TLS.Certificates[0]
	certificateServer.Close()
	listener, err := net.ListenPacket("udp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	server := relay.transportServer(&tls.Config{MinVersion: tls.VersionTLS13, Certificates: []tls.Certificate{certificate}}, listener.LocalAddr().String())
	done := make(chan error, 1)
	serveCtx, stopServe := context.WithCancel(t.Context())
	go func() { done <- relay.serveQUIC(serveCtx, listener, server) }()
	defer func() { stopServe(); <-done }()
	ctx, cancel := context.WithTimeout(t.Context(), testwait.DefaultTimeout)
	defer cancel()
	client := &webtransport.Transport{TLSClientConfig: &tls.Config{InsecureSkipVerify: true}} // Only the local test certificate is untrusted.
	defer client.Close()
	response, session, err := client.Dial(ctx, "https://"+listener.LocalAddr().String()+bridgecontract.RelayTransportPath, nil)
	if err == nil {
		_ = session.CloseWithError(0, "")
		t.Fatal("a WebTransport session opened without an admission")
	}
	if response == nil || response.StatusCode != http.StatusForbidden {
		t.Fatal("WebTransport without an admission was not refused before Upgrade", err)
	}
}

func TestScopeDeadlinePassedDuringResolutionIsRefused(t *testing.T) {
	revoked, cancel := context.WithCancel(context.Background())
	defer cancel()
	// The resolver answers only after the admission it returns has expired.
	slow := func(*http.Request) (Scope, error) {
		deadline := time.Now().Add(20 * time.Millisecond)
		<-time.After(time.Until(deadline) + time.Millisecond)
		return Scope{ID: "A", Deadline: deadline, Revoked: revoked}, nil
	}
	relay := scopeRelay(t, Config{ResolveScope: slow}, nil)
	host := newScopeHost(t, relay.Handler())
	if conn, status := host.dial(t, bridgecontract.RelayChannelControl, ""); conn != nil || status != http.StatusForbidden {
		t.Fatalf("an admission that expired while it was resolved upgraded with status %d", status)
	}
}

func TestCloseWaitsForAReplacedScopedSession(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		relay := New(Config{Logger: slog.New(slog.DiscardHandler)})
		scope, _ := testScope(t, "A", time.Now().Add(time.Hour))
		previous, detach, err := relay.openControl(scope, netip.MustParseAddr("192.0.2.1"), "", time.Now(), testBrowserUserAgent, false)
		if err != nil {
			t.Fatal(err)
		}
		defer func() { detach(time.Now()) }()
		session := previous.session
		if _, err := session.beginPlay(t.Context(), previous.ctx, 0); err != nil {
			t.Fatal(err)
		}
		// A fresh control forgets the session; Close begins before the scope watcher runs.
		relay.mu.Lock()
		relay.forgetLocked(session)
		relay.cancel()
		relay.mu.Unlock()
		closed := make(chan struct{})
		go func() { relay.Close(); close(closed) }()
		synctest.Wait()
		select {
		case <-closed:
			t.Fatal("Close returned while the replaced session was still playing")
		default:
		}
		session.endPlay()
		select {
		case <-closed:
		case <-time.After(time.Minute):
			t.Fatal("Close never returned after the replaced session's play ended")
		}
		session.mu.Lock()
		defer session.mu.Unlock()
		if !session.closed {
			t.Fatal("the replaced session's account and handler were never closed")
		}
	})
}
