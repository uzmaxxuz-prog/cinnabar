package webrelay

import (
	"log/slog"
	"net/netip"
	"testing"
	"time"
)

func TestStage1PerIPBudgetsAllowTwoJoinsAndTwoRejoinsWithoutRefill(t *testing.T) {
	relay := New(Config{
		Logger:              slog.New(slog.DiscardHandler),
		MaxConnectionsPerIP: 8,
		MaxSessionsPerIP:    8,
		ActionRatePerMinute: 12,
		ActionBurst:         12,
	})
	defer relay.Close()
	client := netip.MustParseAddr("198.51.100.10")
	now := time.Unix(1_700_000_000, 0)

	// Two offline joins + two immediate reconnects = 4 * 3 = 12 action tokens without refill.
	var releases []func()
	for i := range 8 {
		release, err := relay.admit(client, now)
		if err != nil {
			t.Fatalf("admit %d failed: %v", i, err)
		}
		releases = append(releases, release)
	}
	if _, err := relay.admit(client, now); err == nil {
		t.Fatal("expected 9th simultaneous connection to be refused")
	}
	// Remaining 4 action tokens from burst 12 succeed while 8 connections are held.
	for i := range 4 {
		relay.mu.Lock()
		ok := relay.allowLocked(client, now)
		relay.mu.Unlock()
		if !ok {
			t.Fatalf("action %d within burst 12 refused", i+9)
		}
	}
	relay.mu.Lock()
	if relay.allowLocked(client, now) {
		relay.mu.Unlock()
		t.Fatal("expected 13th action without refill to be refused")
	}
	relay.mu.Unlock()
	for _, release := range releases {
		release()
	}
}
