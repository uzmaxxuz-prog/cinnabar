package webrelay

import (
	"bufio"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/netip"
	"os"
	"slices"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// The network egress suite runs this package's test binary in two containers on one internal
// network: the game fixture at the configured game endpoint and the relay beside it. It proves the
// policy with real private addresses, a real resolver name for the game and listeners the relay
// must not reach. Outside that network the suite skips.
const (
	egressRoleEnv     = "WEBRELAY_EGRESS_ROLE"      // "game" or "relay"
	egressGameEnv     = "WEBRELAY_EGRESS_GAME"      // the game endpoint, an IP and port
	egressGameNameEnv = "WEBRELAY_EGRESS_GAME_NAME" // a resolver name of the game host
	egressSelfEnv     = "WEBRELAY_EGRESS_SELF"      // the relay host's own private IP
)

// egressNetwork reads the suite's configuration for role, or skips outside the suite.
func egressNetwork(t *testing.T, role string) netip.AddrPort {
	t.Helper()
	if os.Getenv(egressRoleEnv) != role {
		t.Skipf("missing fixture: %s=%s and %s are set only inside the Docker egress network", egressRoleEnv, role, egressGameEnv)
	}
	game, err := netip.ParseAddrPort(os.Getenv(egressGameEnv))
	if err != nil {
		t.Fatalf("%s: %v", egressGameEnv, err)
	}
	return game
}

// egressFixturePorts are the game host's probe and report ports beside the game port.
func egressFixturePorts(game netip.AddrPort) (probe, report netip.AddrPort) {
	return netip.AddrPortFrom(game.Addr(), game.Port()+1), netip.AddrPortFrom(game.Addr(), game.Port()+2)
}

// egressEvent is one thing that reached the game host other than a game join.
type egressEvent struct {
	Kind string `json:"kind"` // "udp", "tcp" or "join"
	Port uint16 `json:"port"`
	Data string `json:"data"`
}

// egressRecorder collects the game host's events in arrival order.
type egressRecorder struct {
	mu     sync.Mutex
	events []egressEvent
}

func (recorder *egressRecorder) add(event egressEvent) {
	recorder.mu.Lock()
	recorder.events = append(recorder.events, event)
	recorder.mu.Unlock()
}

func (recorder *egressRecorder) snapshot() []egressEvent {
	recorder.mu.Lock()
	defer recorder.mu.Unlock()
	return slices.Clone(recorder.events)
}

// TestNetworkEgressGameFixture serves the game endpoint, a UDP and TCP probe on the next port, a TCP
// probe on the game port itself and a report port, until the relay suite sends stop.
func TestNetworkEgressGameFixture(t *testing.T) {
	game := egressNetwork(t, "game")
	probe, report := egressFixturePorts(game)
	recorder := new(egressRecorder)
	upstream := newScopeUpstreamAt(t, game.String())
	var latest *minecraft.Conn
	var latestMu sync.Mutex
	go func() {
		for server := range upstream.joined {
			latestMu.Lock()
			latest = server
			latestMu.Unlock()
			recorder.add(egressEvent{Kind: "join", Port: game.Port()})
		}
	}()
	datagrams, err := net.ListenUDP("udp", net.UDPAddrFromAddrPort(probe))
	if err != nil {
		t.Fatal(err)
	}
	defer datagrams.Close()
	go func() {
		buffer := make([]byte, 2048)
		for {
			n, _, err := datagrams.ReadFromUDP(buffer)
			if err != nil {
				return
			}
			recorder.add(egressEvent{Kind: "udp", Port: probe.Port(), Data: string(buffer[:n])})
		}
	}()
	for _, address := range []netip.AddrPort{game, probe} {
		listener, err := net.ListenTCP("tcp", net.TCPAddrFromAddrPort(address))
		if err != nil {
			t.Fatal(err)
		}
		defer listener.Close()
		go func() {
			for {
				conn, err := listener.Accept()
				if err != nil {
					return
				}
				_ = conn.SetReadDeadline(time.Now().Add(testwait.DefaultTimeout))
				line, _ := bufio.NewReader(conn).ReadString('\n')
				_ = conn.Close()
				recorder.add(egressEvent{Kind: "tcp", Port: address.Port(), Data: strings.TrimSpace(line)})
			}
		}()
	}
	reports, err := net.ListenTCP("tcp", net.TCPAddrFromAddrPort(report))
	if err != nil {
		t.Fatal(err)
	}
	defer reports.Close()
	for {
		conn, err := reports.Accept()
		if err != nil {
			t.Fatal(err)
		}
		command, _ := bufio.NewReader(conn).ReadString('\n')
		switch strings.TrimSpace(command) {
		case "events":
			_ = json.NewEncoder(conn).Encode(recorder.snapshot())
		case "transfer":
			latestMu.Lock()
			server := latest
			latestMu.Unlock()
			if server != nil && server.WritePacket(&packet.Transfer{Address: probe.Addr().String(), Port: probe.Port()}) == nil && server.Flush() == nil {
				_, _ = io.WriteString(conn, "ok\n")
			}
		case "stop":
			_ = conn.Close()
			return
		}
		_ = conn.Close()
	}
}

// egressFixture is the relay's view of the game host.
type egressFixture struct {
	game, probe, report netip.AddrPort
	sentinels           int
}

// command sends one report command and returns the fixture's answer.
func (fixture *egressFixture) command(command string) ([]byte, error) {
	conn, err := net.DialTimeout("tcp", fixture.report.String(), testwait.DefaultTimeout)
	if err != nil {
		return nil, err
	}
	defer conn.Close()
	_ = conn.SetDeadline(time.Now().Add(testwait.DefaultTimeout))
	if _, err = io.WriteString(conn, command+"\n"); err != nil {
		return nil, err
	}
	return io.ReadAll(conn)
}

// events returns everything that has reached the game host so far.
func (fixture *egressFixture) events(t *testing.T) []egressEvent {
	t.Helper()
	answer, err := fixture.command("events")
	if err != nil {
		t.Fatal(err)
	}
	var events []egressEvent
	if err := json.Unmarshal(answer, &events); err != nil {
		t.Fatal(err)
	}
	return events
}

// joins counts the game joins the game host accepted.
func (fixture *egressFixture) joins(t *testing.T) int {
	t.Helper()
	count := 0
	for _, event := range fixture.events(t) {
		if event.Kind == "join" {
			count++
		}
	}
	return count
}

// requireSilent sends sentinels to every game-host probe and requires the sentinels to be the only
// datagrams and connections the probes ever received.
func (fixture *egressFixture) requireSilent(t *testing.T) {
	t.Helper()
	fixture.sentinels++
	sentinel := fmt.Sprintf("egress-sentinel-%d", fixture.sentinels)
	sender, err := net.DialUDP("udp", nil, net.UDPAddrFromAddrPort(fixture.probe))
	if err != nil {
		t.Fatal(err)
	}
	defer sender.Close()
	if _, err = sender.Write([]byte(sentinel)); err != nil {
		t.Fatal(err)
	}
	for _, address := range []netip.AddrPort{fixture.game, fixture.probe} {
		conn, err := net.DialTimeout("tcp", address.String(), testwait.DefaultTimeout)
		if err != nil {
			t.Fatal(err)
		}
		_, err = io.WriteString(conn, sentinel+"\n")
		_ = conn.Close()
		if err != nil {
			t.Fatal(err)
		}
	}
	var events []egressEvent
	testwait.Eventually(t, testwait.DefaultTimeout, "the game host's sentinels", func() bool {
		events = fixture.events(t)
		seen := 0
		for _, event := range events {
			if event.Data == sentinel {
				seen++
			}
		}
		return seen == 3
	})
	for _, event := range events {
		if event.Kind != "join" && !strings.HasPrefix(event.Data, "egress-sentinel-") {
			t.Fatalf("a refused dial reached the game host: %+v", event)
		}
	}
}

// TestNetworkEgress drives the relay against the game fixture with the stage's real addresses.
func TestNetworkEgress(t *testing.T) {
	game := egressNetwork(t, "relay")
	name := os.Getenv(egressGameNameEnv)
	self, err := netip.ParseAddr(os.Getenv(egressSelfEnv))
	if name == "" || err != nil {
		t.Fatalf("%s and %s must name the game host and the relay's own IP", egressGameNameEnv, egressSelfEnv)
	}
	probe, report := egressFixturePorts(game)
	fixture := &egressFixture{game: game, probe: probe, report: report}
	testwait.Eventually(t, testwait.DefaultTimeout, "the game fixture", func() bool {
		_, err := fixture.command("events")
		return err == nil
	})
	defer func() { _, _ = fixture.command("stop") }()
	port := strconv.Itoa(int(game.Port()))
	ownHost := newEgressProbe(t, net.JoinHostPort(self.String(), port))
	loop := newEgressProbe(t, net.JoinHostPort("127.0.0.1", port))

	t.Run("default policy refuses the private game", func(t *testing.T) {
		relay, host := gameRelay(t, netip.AddrPort{})
		browser := openGameBrowser(t, relay, host)
		browser.requireRefused(t, game.String(), ownHost, loop)
		if joins := fixture.joins(t); joins != 0 {
			t.Fatalf("the default policy joined the private game %d times", joins)
		}
		fixture.requireSilent(t)
	})

	relay, host := gameRelay(t, game)
	browser := openGameBrowser(t, relay, host)
	t.Run("other targets are refused", func(t *testing.T) {
		for _, target := range []string{
			probe.String(),
			net.JoinHostPort(self.String(), port),
			net.JoinHostPort("127.0.0.1", port),
			net.JoinHostPort(name, port),
			net.JoinHostPort("::ffff:"+game.Addr().String(), port),
			net.JoinHostPort("169.254.169.254", "80"),
			net.JoinHostPort("10.0.0.1", port),
			net.JoinHostPort("8.8.8.8", port),
		} {
			browser.requireRefused(t, target, ownHost, loop)
		}
		if joins := fixture.joins(t); joins != 0 {
			t.Fatalf("a refused target joined the game %d times", joins)
		}
		fixture.requireSilent(t)
	})

	t.Run("dialer holds sockets to the endpoint", func(t *testing.T) {
		for _, address := range []string{probe.String(), net.JoinHostPort(name, port), net.JoinHostPort("::ffff:"+game.Addr().String(), port), net.JoinHostPort(self.String(), port)} {
			if conn, err := relay.dialer.DialContext(t.Context(), "udp", address); !errors.Is(err, errOutsideGame) {
				if conn != nil {
					_ = conn.Close()
				}
				t.Errorf("game dial to %s = %v, want a refusal outside the game endpoint", address, err)
			}
		}
		for _, address := range []string{game.String(), net.JoinHostPort(name, port), probe.String()} {
			if conn, err := relay.dialer.DialContext(t.Context(), "tcp", address); !errors.Is(err, errBlockedUpstream) {
				if conn != nil {
					_ = conn.Close()
				}
				t.Errorf("HTTP dial to %s = %v, want the public-only refusal", address, err)
			}
		}
		fixture.requireSilent(t)
	})

	t.Run("the endpoint joins and a transfer outside it ends the session", func(t *testing.T) {
		play, conn := browser.requireJoined(t, game.String())
		testwait.Eventually(t, testwait.DefaultTimeout, "the game join", func() bool { return fixture.joins(t) == 1 })
		if reply, err := fixture.command("transfer"); err != nil || strings.TrimSpace(string(reply)) != "ok" {
			t.Fatalf("the game could not send a transfer: %q %v", reply, err)
		}
		if kind := answer(t, play); kind != bridgecontract.SessionKindDisconnect {
			t.Fatalf("a transfer outside the game endpoint was answered with kind %d, want a disconnect", kind)
		}
		browser.requireEnded(t, conn)
		browser.requireRefused(t, "", ownHost, loop)
		fixture.requireSilent(t)
	})
}
