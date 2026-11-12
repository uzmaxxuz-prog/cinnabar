package webrelay

import (
	"context"
	"errors"
	"net/http"
	"time"
)

// errScope refuses a request or session whose admission scope is missing, revoked or expired.
var errScope = errors.New("webrelay: admission scope refused")

// Scope is one server-side admission that an embedding server verified for a relay request.
// The relay never reads it from browser data. A session keeps the scope it was created with:
// later requests with the same ID can resume it but never extend its deadline.
type Scope struct {
	// ID names the admission opaquely; resume and replacement stay within one ID.
	ID string
	// Deadline is the admission's absolute expiry.
	Deadline time.Time
	// Revoked is cancelled when the embedding server revokes the admission. It must outlive
	// the HTTP request, so it is never the request context.
	Revoked context.Context
}

// valid reports whether the scope names an admission that is still in force at now.
func (scope Scope) valid(now time.Time) bool {
	return scope.ID != "" && scope.Revoked != nil && scope.Revoked.Err() == nil && now.Before(scope.Deadline)
}

// requestScope returns the request's admission. Without a resolver every request is unscoped.
// A resolver error or a scope that is not in force refuses the request before Upgrade.
func (relay *Relay) requestScope(r *http.Request, now time.Time) (Scope, error) {
	if relay.cfg.ResolveScope == nil {
		return Scope{}, nil
	}
	scope, err := relay.cfg.ResolveScope(r)
	if err != nil || !scope.valid(now) {
		return Scope{}, errScope
	}
	return scope, nil
}

// scopeContext ends with parent, or for a scoped request with the scope's revocation or deadline.
// The returned cancel also stops watching the revocation.
func scopeContext(parent context.Context, scope Scope) (context.Context, context.CancelFunc) {
	if scope.ID == "" {
		return context.WithCancel(parent)
	}
	ctx, cancel := context.WithDeadline(parent, scope.Deadline)
	stop := context.AfterFunc(scope.Revoked, cancel)
	return ctx, func() {
		stop()
		cancel()
	}
}

// retire closes a scoped session once its scope ends or a fresh control replaced it: its bearers
// and quota go first, then its channels and upstream end with the session context.
func (relay *Relay) retire(session *webSession) {
	relay.mu.Lock()
	if relay.ctx.Err() != nil {
		// Close cancels every session and erases it after the transports end.
		relay.mu.Unlock()
		return
	}
	relay.forgetLocked(session)
	relay.conns.Add(1)
	relay.mu.Unlock()
	defer relay.conns.Done()
	session.close()
}
