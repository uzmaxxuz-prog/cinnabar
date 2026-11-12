package webrelay

import (
	"context"
	"time"
)

// Scope is one server-side admission that an embedding server verified for a relay request.
// The relay never reads it from browser data.
type Scope struct {
	// ID names the admission opaquely; resume and replacement stay within one ID.
	ID string
	// Deadline is the admission's absolute expiry.
	Deadline time.Time
	// Revoked is cancelled when the embedding server revokes the admission. It must outlive
	// the HTTP request, so it is never the request context.
	Revoked context.Context
}
