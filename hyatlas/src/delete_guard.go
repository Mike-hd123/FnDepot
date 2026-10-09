package main

import (
	"fmt"
	"log"
	"strings"

	"github.com/tuancookiez-hub/hyatlas-v4/memory"
)

// delete_guard.go is the server-side policy behind POST /api/v1/delete_all.
//
// Why it exists: until 4.5.0-2 the endpoint refused only a call that named no
// scope at all. Any single filter was executed immediately, and on this appliance
// `user_id=default` is effectively the whole store — a probe request of exactly
// that shape removed 104,223 rows on 2026-10-09. The guard was not broken by the
// 4.5.0 sync; its granularity was too coarse for a store that cannot be
// regenerated.
//
// The policy lives here as one function of the request plus the store's match
// count, so the handler and the tests cannot drift apart, and so a preview uses
// the same target-selection code as the deletion itself (store.deleteTargets).

// deleteTier is how much scope a request declares, in increasing blast radius.
type deleteTier int

const (
	// tierNarrow names explicit ids. This is the shape plugins/hy_memory's
	// memory_forget uses, so it is never blocked for being wide or for needing
	// the admin switch; only the size gate applies to it.
	tierNarrow deleteTier = iota
	// tierScoped narrows to a real slice: a layer, or both owner keys.
	tierScoped
	// tierWide names one owner key and nothing else. On a single-owner store the
	// owner key is the whole database wearing a scope, which is the 10-09 shape.
	tierWide
	// tierWipe is all=true / confirm=wipe-all, or a call that names no scope.
	tierWipe
)

func (t deleteTier) String() string {
	switch t {
	case tierNarrow:
		return "narrow"
	case tierScoped:
		return "scoped"
	case tierWide:
		return "wide"
	default:
		return "wipe"
	}
}

// deleteScope is the parsed request. DryRun follows the maintenance.go convention:
// absent means true, so a caller must opt in to changing anything.
type deleteScope struct {
	IDs        []string
	Layer      string
	UserID     string
	AgentID    string
	All        bool
	DryRun     bool
	MassOK     bool // confirm_mass_delete=true
	Max        int  // HYATLAS_DELETE_MAX
	Admin      bool // HYATLAS_ADMIN=on
	RemoteAddr string
	UserAgent  string
}

// deleteDecision is the guard's verdict, and the body the handler returns.
// WillDelete is filled on the dry run and on a blocked write alike, so a caller
// that was refused can read the blast radius it was refused for.
type deleteDecision struct {
	Status     int
	DryRun     bool
	Tier       deleteTier
	ScopeDesc  string
	WillDelete int
	Deleted    int
	Max        int
	OverCap    bool
	NotFound   []string
	Error      string
}

// classifyTier maps a declared scope onto a tier using only what the caller named,
// never how many rows happen to match, so the answer does not move as the store
// grows. Size is a separate gate (see deleteGuard), and the two are deliberately
// independent: a wide scope that happens to match three rows is still wide, and a
// scoped request that matches 70,000 rows is still caught.
func classifyTier(sc deleteScope) deleteTier {
	owner := 0
	if sc.UserID != "" {
		owner++
	}
	if sc.AgentID != "" {
		owner++
	}
	switch {
	case sc.All:
		return tierWipe
	case len(sc.IDs) == 0 && sc.Layer == "" && owner == 0:
		// No scope at all: an unguarded call must not reach the wipe path.
		return tierWipe
	case len(sc.IDs) > 0:
		return tierNarrow
	case sc.Layer == "" && owner == 1:
		// user_id alone, or agent_id alone.
		return tierWide
	default:
		return tierScoped
	}
}

// deleteGuard applies the gates in order.
//
// 1. wipe   all=true / no scope at all      -> HYATLAS_ADMIN=on, even to preview
// 2. wide   one owner key alone             -> HYATLAS_ADMIN=on before a write
// 3. size   matches > HYATLAS_DELETE_MAX    -> confirm_mass_delete=true
// 4. wide+write                             -> confirm_mass_delete=true as well
//
// Admin is required for a wipe before anything is even counted: it is the one case
// no amount of confirmation should reach without the switch. A wide scope and an
// over-cap request may both preview, because counting rows hurts nothing and the
// caller needs the number to decide what to do.
//
// memory_forget is unaffected by gates 1, 2 and 4: it names explicit ids, so it is
// tierNarrow, and its batches are 100 ids (plugins/hy_memory/client.py), under the
// default cap of 1000. Only an unusually large id list meets gate 3, and that is
// exactly the case where a second confirmation is the point.
func (s *Server) deleteGuard(sc deleteScope) deleteDecision {
	tier := classifyTier(sc)
	d := deleteDecision{DryRun: sc.DryRun, Tier: tier, Max: sc.Max, Status: 200}
	d.ScopeDesc = describeDeleteScope(sc, tier)

	// Gate 1.
	if tier == tierWipe && !sc.Admin {
		d.Status = 403
		d.Error = "unscoped delete refused: pass layer/user_id/agent_id/id, or set HYATLAS_ADMIN=on to wipe the entire store"
		return d
	}

	n := s.store.DeleteMatchCount(sc.IDs, memory.Layer(sc.Layer), sc.UserID, sc.AgentID)
	d.WillDelete = n
	if tier == tierNarrow && len(sc.IDs) > n {
		d.NotFound = s.missingDeleteIDs(sc.IDs)
	}

	// Gate 2.
	if tier == tierWide && !sc.Admin && !sc.DryRun {
		d.Status = 403
		d.Error = "wide delete scope needs HYATLAS_ADMIN=on: naming one owner key alone matches the whole store on a single-owner server. Narrow it with id=..., or add both user_id and agent_id."
		return d
	}

	// Gate 3: the size cap. A preview is never refused for size - counting rows is
	// the whole point of a dry run, and the caller needs the number to decide. Only
	// a write over the cap needs its own confirmation string.
	if sc.Max > 0 && n > sc.Max {
		d.OverCap = true
		if !sc.DryRun && !sc.MassOK {
			d.Status = 429
			d.Error = fmt.Sprintf("delete would remove %d rows, above HYATLAS_DELETE_MAX=%d; retry with confirm_mass_delete=true if this scope is really intended", n, sc.Max)
			return d
		}
	}

	if sc.DryRun {
		return d
	}

	// Gate 4: a write on a wide scope needs the mass confirmation as well as the
	// admin switch. This is the 10-09 shape exactly — a legal-looking filter,
	// executed for real, that cannot be told apart from wiping the store.
	if tier == tierWide && !sc.MassOK {
		d.Status = 428
		d.Error = "writing delete on a wide scope needs confirm_mass_delete=true: the scope names one key only and cannot be distinguished from wiping the store"
		return d
	}

	deleted, err := s.store.Delete(sc.IDs, memory.Layer(sc.Layer), sc.UserID, sc.AgentID)
	d.Deleted = deleted
	if err != nil {
		d.Status = 500
		d.Error = err.Error()
	}
	return d
}

// describeDeleteScope renders the scope for the audit line without dumping a whole
// id list: an auditor needs the who and the how many, not 100 row keys.
func describeDeleteScope(sc deleteScope, tier deleteTier) string {
	parts := []string{}
	switch {
	case len(sc.IDs) == 1:
		parts = append(parts, "id="+sc.IDs[0])
	case len(sc.IDs) > 1:
		parts = append(parts, fmt.Sprintf("ids=%d", len(sc.IDs)))
	}
	if sc.Layer != "" {
		parts = append(parts, "layer="+sc.Layer)
	}
	if sc.UserID != "" {
		parts = append(parts, "user_id="+sc.UserID)
	}
	if sc.AgentID != "" {
		parts = append(parts, "agent_id="+sc.AgentID)
	}
	if sc.All {
		parts = append(parts, "all=true")
	}
	if len(parts) == 0 {
		parts = append(parts, "unscoped")
	}
	return strings.Join(append(parts, "tier="+tier.String()), " ")
}

// missingDeleteIDs names up to 10 requested ids the store does not have. Reporting
// these is new in 4.5.0-2: a batch where one row was consolidated away used to
// delete the rest in silence, which made an ops script's count untrustworthy. The
// matched ones are still deleted — refusing the batch would break retry of an
// idempotent forget.
func (s *Server) missingDeleteIDs(ids []string) []string {
	var out []string
	for _, id := range ids {
		if _, ok := s.store.GetDoc(id); !ok {
			out = append(out, id)
			if len(out) == 10 {
				break
			}
		}
	}
	return out
}

// auditDelete writes one line per non-dry-run request. The library is not
// regenerable, so every real deletion has to be attributable afterwards: who asked,
// from where, at what scope, and how much went.
func (s *Server) auditDelete(g deleteDecision, sc deleteScope) {
	// A User-Agent is client-controlled and goes into a log line: newline-fold it
	// so one request cannot forge extra log lines.
	ua := sanitizeAuditField(sc.UserAgent)
	if ua == "" {
		ua = "-"
	}
	verdict := "deleted"
	if g.Error != "" {
		verdict = "refused"
	}
	log.Printf("delete_all audit: verdict=%s tier=%s %s matched=%d deleted=%d dry_run=false cap=%d confirm_mass=%v admin=%v remote=%s ua=%q",
		verdict, g.Tier, g.ScopeDesc, g.WillDelete, g.Deleted, sc.Max, sc.MassOK, sc.Admin, auditRemote(sc.RemoteAddr), ua)
}

// sanitizeAuditField strips control characters out of a caller-controlled value
// before it is written to a log line.
func sanitizeAuditField(v string) string {
	var b strings.Builder
	for _, r := range v {
		switch {
		case r == '\n' || r == '\r' || r == '\t':
			b.WriteString(" ")
		case r >= 0x20 && r != 0x7f:
			b.WriteRune(r)
		}
	}
	out := strings.TrimSpace(b.String())
	if len(out) > 160 {
		out = out[:160] + "…"
	}
	return out
}

// auditRemote reduces a RemoteAddr to "host:port". Both v4 and v6 addresses end in a
// port after the last colon, which is all an audit line needs.
func auditRemote(addr string) string {
	if i := strings.LastIndex(addr, ":"); i >= 0 {
		return sanitizeAuditField(addr[:i+1]) + addr[i+1:]
	}
	return sanitizeAuditField(addr)
}
