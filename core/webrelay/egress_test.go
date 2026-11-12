package webrelay

import (
	"bytes"
	"encoding/json"
	"errors"
	"log/slog"
	"net"
	"net/netip"
	"strconv"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// probeSentinel is the payload a probe's own test sender writes; nothing else may reach a probe.
var probeSentinel = []byte("egress-probe-sentinel")

// egressProbe is a control listener: it records what a dial actually delivers to its address, so a
// refusal is proven at the network rather than by an error text.
type egressProbe struct {
	address string
	port    uint16
	udp     *net.UDPConn
	tcp     *net.TCPListener // nil until listenTCP
}

// newEgressProbe receives UDP datagrams on address; port 0 picks a free port.
func newEgressProbe(t *testing.T, address string) *egressProbe {
	t.Helper()
	local, err := net.ResolveUDPAddr("udp", address)
	if err != nil {
		t.Fatal(err)
	}
	conn, err := net.ListenUDP("udp", local)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = conn.Close() })
	bound := conn.LocalAddr().(*net.UDPAddr).AddrPort()
	return &egressProbe{address: bound.String(), port: bound.Port(), udp: conn}
}

// listenTCP also accepts TCP connections on the probe's address and port.
func (probe *egressProbe) listenTCP(t *testing.T) {
	t.Helper()
	listener, err := net.ListenTCP("tcp", net.TCPAddrFromAddrPort(netip.MustParseAddrPort(probe.address)))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	probe.tcp = listener
}

// requireSilent proves that no datagram or connection reached the probe: a sentinel the test sends
// now must be the first thing it receives. Loopback delivery keeps an earlier datagram ahead of it.
func (probe *egressProbe) requireSilent(t *testing.T) {
	t.Helper()
	sender, err := net.DialUDP("udp", nil, probe.udp.LocalAddr().(*net.UDPAddr))
	if err != nil {
		t.Fatal(err)
	}
	defer sender.Close()
	if _, err = sender.Write(probeSentinel); err != nil {
		t.Fatal(err)
	}
	_ = probe.udp.SetReadDeadline(time.Now().Add(testwait.DefaultTimeout))
	buffer := make([]byte, 2048)
	n, from, err := probe.udp.ReadFromUDP(buffer)
	if err != nil {
		t.Fatalf("probe %s received nothing, not even its sentinel: %v", probe.address, err)
	}
	if from.String() != sender.LocalAddr().String() || !bytes.Equal(buffer[:n], probeSentinel) {
		t.Fatalf("a refused dial reached %s from %s", probe.address, from)
	}
	if probe.tcp == nil {
		return
	}
	connection, err := net.DialTCP("tcp", nil, probe.tcp.Addr().(*net.TCPAddr))
	if err != nil {
		t.Fatal(err)
	}
	defer connection.Close()
	_ = probe.tcp.SetDeadline(time.Now().Add(testwait.DefaultTimeout))
	accepted, err := probe.tcp.Accept()
	if err != nil {
		t.Fatalf("probe %s accepted nothing, not even its sentinel: %v", probe.address, err)
	}
	defer accepted.Close()
	if accepted.RemoteAddr().String() != connection.LocalAddr().String() {
		t.Fatalf("a refused HTTP dial reached %s from %s", probe.address, accepted.RemoteAddr())
	}
}

// requireDial waits for a datagram that the test itself did not send.
func (probe *egressProbe) requireDial(t *testing.T) {
	t.Helper()
	_ = probe.udp.SetReadDeadline(time.Now().Add(testwait.DefaultTimeout))
	buffer := make([]byte, 2048)
	n, _, err := probe.udp.ReadFromUDP(buffer)
	if err != nil {
		t.Fatalf("no dial reached %s: %v", probe.address, err)
	}
	if bytes.Equal(buffer[:n], probeSentinel) {
		t.Fatalf("probe %s received only a sentinel", probe.address)
	}
}

// at joins host with a probe's port.
func (probe *egressProbe) at(host string) string {
	return net.JoinHostPort(host, strconv.Itoa(int(probe.port)))
}

// gameRelay serves a library relay whose only game server is endpoint behind a test host.
func gameRelay(t *testing.T, endpoint netip.AddrPort) (*Relay, *scopeHost) {
	t.Helper()
	relay := New(Config{Logger: slog.New(slog.DiscardHandler), GameEndpoint: endpoint})
	t.Cleanup(relay.Close)
	unlimitedActions(relay)
	return relay, newScopeHost(t, relay.Handler())
}

// gameBrowser is one browser's control channel and the capability its plays present.
type gameBrowser struct {
	host    *scopeHost
	relay   *Relay
	control net.Conn
	welcome controlEvent
}

// openGameBrowser opens a fresh control session on host.
func openGameBrowser(t *testing.T, relay *Relay, host *scopeHost) *gameBrowser {
	t.Helper()
	control, _ := host.dial(t, bridgecontract.RelayChannelControl, "")
	return &gameBrowser{host: host, relay: relay, control: control, welcome: scopeControl(t, control, "")}
}

// gameConnect is an offline Connect to target; an empty target sends a targetless Connect.
func gameConnect(t *testing.T, target string) []byte {
	t.Helper()
	frame := scopeConnect(t, target, "Egress")
	if target != "" {
		return frame
	}
	var request map[string]json.RawMessage
	if err := json.Unmarshal(frame[1:], &request); err != nil {
		t.Fatal(err)
	}
	delete(request, "target")
	body, err := json.Marshal(request)
	if err != nil {
		t.Fatal(err)
	}
	return append([]byte{bridgecontract.SessionKindConnect}, body...)
}

// connect sends a Connect to target on a fresh play and returns the core's first answer.
func (browser *gameBrowser) connect(t *testing.T, target string) (byte, *streamnet.FramedConn, net.Conn) {
	t.Helper()
	conn, _ := browser.host.dial(t, bridgecontract.RelayChannelPlay, "")
	if err := writeJSON(t.Context(), conn, hello{Version: bridgecontract.RelayVersion, Channel: bridgecontract.RelayChannelPlay,
		Resume: browser.welcome.Resume, Generation: browser.welcome.Generation}); err != nil {
		t.Fatal(err)
	}
	play := streamnet.NewFramedConn(conn)
	if _, err := play.Write(gameConnect(t, target)); err != nil {
		t.Fatal(err)
	}
	return answer(t, play), play, conn
}

// answer reads play until the core hands off a join or ends the session, and returns that kind.
func answer(t *testing.T, play *streamnet.FramedConn) byte {
	t.Helper()
	for {
		frame, err := play.ReadPacket()
		if err != nil {
			t.Fatal("play ended without an answer", err)
		}
		switch frame[0] {
		case bridgecontract.SessionKindHandoff, bridgecontract.SessionKindDisconnect, bridgecontract.SessionKindTransfer:
			return frame[0]
		}
	}
}

// requireEnded waits until the relay has closed play and released its transport slot.
func (browser *gameBrowser) requireEnded(t *testing.T, conn net.Conn) {
	t.Helper()
	requireClosed(t, conn, "ended play")
	awaitTransports(t, browser.relay, 1)
}

// requireRefused requires a disconnect for a Connect to target, then proves with every probe that
// no connection reached it.
func (browser *gameBrowser) requireRefused(t *testing.T, target string, probes ...*egressProbe) {
	t.Helper()
	kind, _, conn := browser.connect(t, target)
	if kind != bridgecontract.SessionKindDisconnect {
		t.Fatalf("a Connect to %s was answered with kind %d, want a disconnect", target, kind)
	}
	browser.requireEnded(t, conn)
	for _, probe := range probes {
		probe.requireSilent(t)
	}
}

// requireJoined requires a Connect to target to hand off a join.
func (browser *gameBrowser) requireJoined(t *testing.T, target string) (*streamnet.FramedConn, net.Conn) {
	t.Helper()
	kind, play, conn := browser.connect(t, target)
	if kind != bridgecontract.SessionKindHandoff {
		t.Fatalf("a Connect to %s was answered with kind %d, want a handoff", target, kind)
	}
	return play, conn
}

// The library's default policy admits public game servers only, so a private game such as the
// stage's own server is refused before any datagram leaves the relay.
func TestDefaultEgressRefusesPrivateGame(t *testing.T) {
	game := newEgressProbe(t, "127.0.0.1:0")
	relay, host := gameRelay(t, netip.AddrPort{})
	browser := openGameBrowser(t, relay, host)

	browser.requireRefused(t, game.address, game)
}

func TestGameEndpointJoinsItsGame(t *testing.T) {
	upstream := newScopeUpstream(t)
	relay, host := gameRelay(t, netip.MustParseAddrPort(upstream.address))
	browser := openGameBrowser(t, relay, host)

	browser.requireJoined(t, upstream.address)
	upstream.requireJoin(t)
}

// A configured endpoint is the only Connect target: every other host, port, name or spelling is
// refused, and no datagram reaches the endpoint or any other listener on the way.
func TestGameEndpointRefusesEveryOtherTarget(t *testing.T) {
	game := newEgressProbe(t, "127.0.0.1:0")
	otherPort := newEgressProbe(t, "127.0.0.1:0")
	otherHost := newEgressProbe(t, game.at("127.0.0.2"))
	relay, host := gameRelay(t, netip.MustParseAddrPort(game.address))
	browser := openGameBrowser(t, relay, host)

	for _, target := range []string{
		otherPort.address,
		otherHost.address,
		game.at("localhost"),
		game.at("::ffff:127.0.0.1"),
		game.at("::1"),
		game.at("0.0.0.0"),
		game.at("10.0.0.1"),
		game.at("169.254.169.254"),
		game.at("8.8.8.8"),
	} {
		browser.requireRefused(t, target, game, otherPort, otherHost)
	}

	kind, _, _ := browser.connect(t, game.address)
	if kind == bridgecontract.SessionKindDisconnect {
		t.Fatal("the endpoint itself was refused")
	}
	game.requireDial(t)
}

// The relay's dialer opens every session socket, transfer hops and redials included, so it holds
// game sockets to the endpoint itself and keeps HTTP, such as pack downloads, off the endpoint.
func TestGameDialerReachesOnlyItsEndpoint(t *testing.T) {
	game := newEgressProbe(t, "127.0.0.1:0")
	game.listenTCP(t)
	otherPort := newEgressProbe(t, "127.0.0.1:0")
	otherHost := newEgressProbe(t, game.at("127.0.0.2"))
	relay := New(Config{Logger: slog.New(slog.DiscardHandler), GameEndpoint: netip.MustParseAddrPort(game.address)})
	t.Cleanup(relay.Close)

	for _, address := range []string{
		otherPort.address,
		otherHost.address,
		game.at("localhost"),
		game.at("::ffff:127.0.0.1"),
		game.at("0.0.0.0"),
		game.at("8.8.8.8"),
	} {
		if conn, err := relay.dialer.DialContext(t.Context(), "udp", address); !errors.Is(err, errOutsideGame) {
			if conn != nil {
				_ = conn.Close()
			}
			t.Errorf("game dial to %s = %v, want a refusal outside the game endpoint", address, err)
		}
	}
	if conn, err := relay.dialer.DialContext(t.Context(), "tcp", game.address); !errors.Is(err, errBlockedUpstream) {
		if conn != nil {
			_ = conn.Close()
		}
		t.Errorf("an HTTP dial to the game endpoint = %v, want the public-only refusal", err)
	}
	game.requireSilent(t)

	conn, err := relay.dialer.DialContext(t.Context(), "udp", game.address)
	if err != nil {
		t.Fatal("the game dial to its endpoint was refused", err)
	}
	defer conn.Close()
	if _, err = conn.Write([]byte("game")); err != nil {
		t.Fatal(err)
	}
	game.requireDial(t)
}

// An operator may write the endpoint as IPv4-mapped IPv6; it still names the IPv4 game socket.
func TestGameEndpointConfiguredAsMappedIPv4(t *testing.T) {
	game := newEgressProbe(t, "127.0.0.1:0")
	relay, host := gameRelay(t, netip.MustParseAddrPort(game.at("::ffff:127.0.0.1")))
	browser := openGameBrowser(t, relay, host)

	kind, _, _ := browser.connect(t, game.address)
	if kind == bridgecontract.SessionKindDisconnect {
		t.Fatal("the endpoint was refused when configured as IPv4-mapped IPv6")
	}
	game.requireDial(t)
}

// A server transfer outside the endpoint ends the session instead of moving it; a transfer to the
// endpoint itself still works.
func TestGameEndpointRefusesTransferOutsideIt(t *testing.T) {
	upstream := newScopeUpstream(t)
	outside := newEgressProbe(t, "127.0.0.1:0")
	endpoint := netip.MustParseAddrPort(upstream.address)
	relay, host := gameRelay(t, endpoint)
	browser := openGameBrowser(t, relay, host)
	play, conn := browser.requireJoined(t, upstream.address)
	server := upstream.requireJoin(t)

	if err := server.WritePacket(&packet.Transfer{Address: "127.0.0.1", Port: outside.port}); err != nil {
		t.Fatal(err)
	}
	if err := server.Flush(); err != nil {
		t.Fatal(err)
	}
	if kind := answer(t, play); kind != bridgecontract.SessionKindDisconnect {
		t.Fatalf("a transfer outside the game endpoint was answered with kind %d, want a disconnect", kind)
	}
	browser.requireEnded(t, conn)
	upstream.requireLeft(t, server)
	browser.requireRefused(t, "", outside)

	play, conn = browser.requireJoined(t, upstream.address)
	server = upstream.requireJoin(t)
	if err := server.WritePacket(&packet.Transfer{Address: endpoint.Addr().String(), Port: endpoint.Port()}); err != nil {
		t.Fatal(err)
	}
	if err := server.Flush(); err != nil {
		t.Fatal(err)
	}
	if kind := answer(t, play); kind != bridgecontract.SessionKindTransfer {
		t.Fatalf("a transfer to the game endpoint was answered with kind %d, want a transfer", kind)
	}
	browser.requireEnded(t, conn)
	browser.requireJoined(t, "")
	upstream.requireJoin(t)
}

// A transfer the server sends during login is a hop the core would follow before the handoff; it
// dials through the same policy, so a hop outside the endpoint sends nothing.
func TestGameEndpointRefusesPreLoginTransferHop(t *testing.T) {
	outside := newEgressProbe(t, "127.0.0.1:0")
	listener, err := minecraft.ListenConfig{AuthenticationDisabled: true, ErrorLog: slog.New(slog.DiscardHandler)}.Listen("raknet", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	go func() {
		for {
			accepted, err := listener.Accept()
			if err != nil {
				return
			}
			server := accepted.(*minecraft.Conn)
			_ = server.WritePacket(&packet.Transfer{Address: "127.0.0.1", Port: outside.port})
			_ = server.Flush()
		}
	}()
	relay, host := gameRelay(t, netip.MustParseAddrPort(listener.Addr().String()))
	browser := openGameBrowser(t, relay, host)

	browser.requireRefused(t, listener.Addr().String(), outside)
}

// A browser that reconnects after a network break resumes the same session; its next Connects are
// held to the endpoint exactly like its first.
func TestGameEndpointHoldsReconnectsToIt(t *testing.T) {
	upstream := newScopeUpstream(t)
	outside := newEgressProbe(t, "127.0.0.1:0")
	relay, host := gameRelay(t, netip.MustParseAddrPort(upstream.address))
	browser := openGameBrowser(t, relay, host)
	_, conn := browser.requireJoined(t, upstream.address)
	server := upstream.requireJoin(t)

	_ = browser.control.Close()
	_ = conn.Close()
	upstream.requireLeft(t, server)
	awaitTransports(t, relay, 0)
	control, _ := host.dial(t, bridgecontract.RelayChannelControl, "")
	resumed := scopeControl(t, control, browser.welcome.Resume)
	if resumed.Type != bridgecontract.RelayEventWelcome {
		t.Fatalf("the session did not resume: %+v", resumed)
	}
	browser.control, browser.welcome = control, resumed

	browser.requireRefused(t, outside.address, outside)
	browser.requireJoined(t, upstream.address)
	upstream.requireJoin(t)
}
