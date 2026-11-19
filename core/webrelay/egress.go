package webrelay

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/netip"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/control"
)

var (
	// errBlockedUpstream marks a dial refused because its resolved destination is not a public host.
	errBlockedUpstream = errors.New("webrelay: upstream address is not public")
	// errOutsideGame marks a game dial refused because it does not name the configured game endpoint.
	errOutsideGame = errors.New("webrelay: upstream address is not the game endpoint")
)

// reservedPrefixes are special-purpose ranges netip's predicates do not cover. NAT64 and 6to4
// prefixes are refused because they embed, and can route to, any IPv4 address.
var reservedPrefixes = []netip.Prefix{
	netip.MustParsePrefix("0.0.0.0/8"),
	netip.MustParsePrefix("100.64.0.0/10"),
	netip.MustParsePrefix("192.0.0.0/24"),
	netip.MustParsePrefix("192.0.2.0/24"),
	netip.MustParsePrefix("192.88.99.0/24"),
	netip.MustParsePrefix("198.18.0.0/15"),
	netip.MustParsePrefix("198.51.100.0/24"),
	netip.MustParsePrefix("203.0.113.0/24"),
	netip.MustParsePrefix("240.0.0.0/4"),
	netip.MustParsePrefix("64:ff9b::/96"),
	netip.MustParsePrefix("64:ff9b:1::/48"),
	netip.MustParsePrefix("100::/64"),
	netip.MustParsePrefix("2001::/23"),
	netip.MustParsePrefix("2001:db8::/32"),
	netip.MustParsePrefix("2002::/16"),
	netip.MustParsePrefix("fec0::/10"),
}

// publicUpstream reports whether addr is a public unicast host, the only kind the public-only
// policy admits.
func publicUpstream(addr netip.Addr) bool {
	addr = addr.Unmap()
	if !addr.IsValid() || !addr.IsGlobalUnicast() || addr.IsPrivate() {
		return false
	}
	for _, prefix := range reservedPrefixes {
		if prefix.Contains(addr) {
			return false
		}
	}
	return true
}

// upstreamDialConnectTimeout bounds one socket setup; RakNet bounds the handshake itself.
const upstreamDialConnectTimeout = 10 * time.Second

// newPublicDialer returns a dialer that reaches public hosts only. Its Control runs after name
// resolution, on the exact address the socket connects to, so a hostname cannot resolve its way
// past the policy, and every redirect hop and reconnection is checked again.
func newPublicDialer() *net.Dialer {
	return &net.Dialer{
		Timeout: upstreamDialConnectTimeout,
		Control: func(_, address string, _ syscall.RawConn) error {
			endpoint, err := netip.ParseAddrPort(address)
			if err != nil {
				return fmt.Errorf("%w: unparsable destination", errBlockedUpstream)
			}
			if !publicUpstream(endpoint.Addr()) {
				return errBlockedUpstream
			}
			return nil
		},
	}
}

// upstreamDialer opens every socket of relay sessions. Game sockets are UDP, which RakNet uses
// for the first join, redials, transfer hops and reconnects alike: without an endpoint they reach
// public hosts only, with one only the endpoint. HTTP connections, such as pack downloads and their
// redirects, are TCP and keep the public-only policy in both modes, so a private endpoint is never
// an HTTP destination while a public endpoint's host is reachable like any public host. Other
// networks are refused.
type upstreamDialer struct {
	// endpoint is the only game server sessions may reach; the zero value admits public servers.
	endpoint netip.AddrPort
	// admitGame decides a game address before resolution and again on the socket's own address.
	admitGame func(address string) error
	game      *net.Dialer
	http      *net.Dialer
}

// newUpstreamDialer returns the relay's dialer for endpoint. An IPv4-mapped endpoint names its IPv4
// address, the form a socket reports.
func newUpstreamDialer(endpoint netip.AddrPort) *upstreamDialer {
	dialer := &upstreamDialer{game: newPublicDialer(), http: newPublicDialer()}
	if !endpoint.Addr().IsValid() {
		return dialer
	}
	dialer.endpoint = netip.AddrPortFrom(endpoint.Addr().Unmap(), endpoint.Port())
	dialer.admitGame = func(address string) error { return admitGameAddress(dialer.endpoint, address) }
	dialer.game = &net.Dialer{
		Timeout: upstreamDialConnectTimeout,
		Control: func(_, address string, _ syscall.RawConn) error { return dialer.admitGame(address) },
	}
	return dialer
}

// DialContext opens a game or HTTP socket under its policy. A game address is checked before any
// name resolution, so a DNS name cannot stand in for the endpoint.
func (dialer *upstreamDialer) DialContext(ctx context.Context, network, address string) (net.Conn, error) {
	switch network {
	case "udp", "udp4", "udp6":
		if dialer.admitGame != nil {
			if err := dialer.admitGame(address); err != nil {
				return nil, err
			}
		}
		return dialer.game.DialContext(ctx, network, address)
	case "tcp", "tcp4", "tcp6":
		return dialer.http.DialContext(ctx, network, address)
	default:
		return nil, fmt.Errorf("%w: network %s", errBlockedUpstream, network)
	}
}

// selectTarget maps a Connect's target to the address its session dials. Without an endpoint it
// admits any host:port RakNet target and the dialer decides whether it is reachable. With one the
// client cannot choose: a target other than the endpoint's own IP and port is invalid, and the
// session dials the relay's endpoint.
func (dialer *upstreamDialer) selectTarget(_ context.Context, kind, value string) (string, error) {
	if kind != control.TargetRakNet {
		return "", control.ErrInvalidTarget
	}
	value = strings.TrimSpace(value)
	if dialer.admitGame != nil {
		if dialer.admitGame(value) != nil {
			return "", control.ErrInvalidTarget
		}
		return dialer.endpoint.String(), nil
	}
	host, port, err := net.SplitHostPort(value)
	number, portErr := strconv.ParseUint(port, 10, 16)
	if err != nil || host == "" || portErr != nil || number == 0 {
		return "", control.ErrInvalidTarget
	}
	return value, nil
}

// allowTransfer decides a server transfer's address: with an endpoint only the endpoint itself.
func (dialer *upstreamDialer) allowTransfer(address string) error {
	if dialer.admitGame == nil {
		return nil
	}
	return dialer.admitGame(address)
}

// admitGameAddress accepts only address written as endpoint's IP and port. Host names, other
// spellings of the IP such as IPv4-mapped IPv6, and every other host or port are refused.
func admitGameAddress(endpoint netip.AddrPort, address string) error {
	parsed, err := netip.ParseAddrPort(address)
	if err != nil {
		return fmt.Errorf("%w: not a literal IP and port", errOutsideGame)
	}
	if parsed.Addr() != endpoint.Addr() || parsed.Port() != endpoint.Port() {
		return errOutsideGame
	}
	return nil
}
