package main

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"testing"

	"github.com/tuancookiez-hub/hyatlas-v4/memory"
)

// delete_guard_test.go covers the four guards added in 4.5.0-2 and the plugin
// compatibility contract they must not break. It uses a small in-memory store
// rather than a production replica: the policy depends on what a request declares
// and how many rows match, so ten rows exercise the same branches as 100,000.

func seedStore(t *testing.T, s *Server, layer memory.Layer, user, agent string, n int) []string {
	t.Helper()
	ids := make([]string, 0, n)
	for i := 0; i < n; i++ {
		id := newID()
		if err := s.store.Add(layer, id, "row "+strings.Repeat("x", 8)+string(rune('a'+i%26)),
			map[string]string{"user_id": user, "agent_id": agent}); err != nil {
			t.Fatal(err)
		}
		ids = append(ids, id)
	}
	return ids
}

// deleteCall issues POST /api/v1/delete_all. qs is a raw query string (repeated
// id= keys are preserved, so id=a&id=b survives). body is a JSON envelope; nil
// stands for an empty body, which is how curl sends it and how the 10-09 probe
// went out.
func deleteCall(t *testing.T, s *Server, qs string, body any) (int, map[string]any) {
	t.Helper()
	var r *http.Request
	if body == nil {
		r = httptest.NewRequest(http.MethodPost, "/api/v1/delete_all?"+qs, strings.NewReader("{}"))
	} else {
		b, err := json.Marshal(body)
		if err != nil {
			t.Fatal(err)
		}
		r = httptest.NewRequest(http.MethodPost, "/api/v1/delete_all?"+qs, strings.NewReader(string(b)))
	}
	r.Host = "127.0.0.1:19528"
	w := httptest.NewRecorder()
	s.handleDelete(w, r)
	var out map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &out); err != nil {
		t.Fatalf("bad json %q: %v", w.Body.String(), err)
	}
	return w.Code, out
}

func expectRows(t *testing.T, s *Server, n int) {
	t.Helper()
	if got := s.store.TotalMemories(); got != n {
		t.Fatalf("store holds %d rows, want %d", got, n)
	}
}

// --- Gate 1: no dry_run=false means nothing is deleted -------------------------

func TestDeleteAllDefaultIsDryRun(t *testing.T) {
	srv := newTestServer(t, "m", "http://127.0.0.1:1/v1")
	seedStore(t, srv, memory.L3Fact, "alice", "a1", 6)

	// A scoped request with an empty body: counted, not deleted.
	code, out := deleteCall(t, srv, "", map[string]any{"user_id": "alice", "agent_id": "a1"})
	if code != 200 {
		t.Fatalf("scoped dry preview: want 200, got %d (%v)", code, out)
	}
	if out["dry_run"] != true {
		t.Errorf("dry_run should default to true, got %v", out["dry_run"])
	}
	if n := int(out["will_delete"].(float64)); n != 6 {
		t.Errorf("will_delete: want 6, got %d", n)
	}
	if n := int(out["deleted_count"].(float64)); n != 0 {
		t.Errorf("a default dry run must not delete, deleted_count=%d", n)
	}
	expectRows(t, srv, 6)

	// A request with no scope at all is a wipe and is refused outright, before
	// the dry-run question even matters. That refusal is preserved from 4.3.0-1.
	code, out = deleteCall(t, srv, "", nil)
	if code != 403 {
		t.Fatalf("unscoped request without admin: want 403, got %d (%v)", code, out)
	}
	if n := int(out["deleted_count"].(float64)); n != 0 {
		t.Errorf("a refused unscoped request deleted %d rows", n)
	}
	expectRows(t, srv, 6)

	// Explicitly dry_run=false deletes, which is what proves the flag gates.
	code, out = deleteCall(t, srv, "", map[string]any{"user_id": "alice", "agent_id": "a1", "dry_run": false})
	if code != 200 {
		t.Errorf("explicit dry_run=false: want 200, got %d (%v)", code, out)
	}
	if n := int(out["deleted_count"].(float64)); n != 6 {
		t.Errorf("explicit dry_run=false should delete 6, got %d", n)
	}
	expectRows(t, srv, 0)
}

func TestDeleteAllEmptyBodyNeverWrites(t *testing.T) {
	srv := newTestServer(t, "m", "http://127.0.0.1:1/v1")
	seedStore(t, srv, memory.L3Fact, "alice", "a1", 3)

	// An empty body carries no scope, so it is a full-store wipe regardless of
	// whether dry_run was spelled true or false. It must never write.
	for _, body := range []any{map[string]any{}, map[string]any{"dry_run": false}} {
		code, out := deleteCall(t, srv, "", body)
		if code != 403 {
			t.Fatalf("empty body %v: want 403, got %d (%v)", body, code, out)
		}
		if n := int(out["deleted_count"].(float64)); n != 0 {
			t.Errorf("empty body %v deleted %d rows", body, n)
		}
	}
	expectRows(t, srv, 3)
}

// --- Gate 2: above HYATLAS_DELETE_MAX a write needs confirm_mass_delete --------

func TestDeleteAllBlocksAboveCapUntilMassConfirm(t *testing.T) {
	srv := newTestServer(t, "m", "http://127.0.0.1:1/v1")
	seedStore(t, srv, memory.L3Fact, "alice", "a1", 5)
	srv.deleteMax = 3
	srv.admin = true

	code, out := deleteCall(t, srv, "", map[string]any{"user_id": "alice", "agent_id": "a1", "dry_run": false})
	if code != 429 {
		t.Fatalf("over-cap write: want 429, got %d (%v)", code, out)
	}
	if !strings.Contains(out["error"].(string), "confirm_mass_delete") {
		t.Errorf("error should name the missing confirmation: %v", out["error"])
	}
	if !strings.Contains(out["error"].(string), "HYATLAS_DELETE_MAX=3") {
		t.Errorf("error should cite the cap: %v", out["error"])
	}
	if n := int(out["deleted_count"].(float64)); n != 0 {
		t.Errorf("blocked write deleted %d rows", n)
	}
	if n := int(out["will_delete"].(float64)); n != 5 {
		t.Errorf("a blocked write should report the blast radius, will_delete=%d", n)
	}
	expectRows(t, srv, 5)

	// With the independent confirmation string it goes through.
	code, out = deleteCall(t, srv, "", map[string]any{
		"user_id": "alice", "agent_id": "a1", "dry_run": false, "confirm_mass_delete": true,
	})
	if code != 200 {
		t.Fatalf("confirmed over-cap write: want 200, got %d (%v)", code, out)
	}
	if n := int(out["deleted_count"].(float64)); n != 5 {
		t.Errorf("deleted_count: want 5, got %d", n)
	}
	expectRows(t, srv, 0)
}

func TestDeleteAllDefaultCapIsConservative(t *testing.T) {
	if defaultDeleteMax != 1000 {
		t.Errorf("defaultDeleteMax: want 1000, got %d", defaultDeleteMax)
	}
	// The plugin's per-request id batch is 100 (plugins/hy_memory/client.py), so
	// the default cap must leave that batch room to work.
	if defaultDeleteMax <= 100 {
		t.Errorf("defaultDeleteMax=%d would break the plugin's 100-id batches", defaultDeleteMax)
	}
}

func TestDeleteAllPreviewIsNeverRefusedForSize(t *testing.T) {
	srv := newTestServer(t, "m", "http://127.0.0.1:1/v1")
	seedStore(t, srv, memory.L3Fact, "alice", "a1", 12)
	srv.deleteMax = 5

	// Reading the blast radius of an over-cap scope is how a caller decides what
	// to do, so it answers 200 with the number and deletes nothing.
	code, out := deleteCall(t, srv, "", map[string]any{"user_id": "alice", "agent_id": "a1"})
	if code != 200 {
		t.Fatalf("over-cap preview: want 200, got %d (%v)", code, out)
	}
	if out["dry_run"] != true {
		t.Errorf("preview should report dry_run=true, got %v", out["dry_run"])
	}
	if n := int(out["will_delete"].(float64)); n != 12 {
		t.Errorf("will_delete: want 12, got %d", n)
	}
	if out["over_cap"] != true {
		t.Errorf("preview should flag over_cap, got %v", out["over_cap"])
	}
	if err, ok := out["error"]; ok && err != nil {
		t.Errorf("a preview must not be an error: %v", err)
	}
	expectRows(t, srv, 12)
}

// --- Gate 3: one owner key alone is a wide scope --------------------------------

func TestDeleteAllWideScopeNeedsAdminAndMassConfirm(t *testing.T) {
	srv := newTestServer(t, "m", "http://127.0.0.1:1/v1")
	seedStore(t, srv, memory.L3Fact, "default", "a1", 4)

	// A preview of a wide scope is allowed: it is the blast-radius read.
	code, out := deleteCall(t, srv, "", map[string]any{"user_id": "default"})
	if code != 200 {
		t.Fatalf("wide dry preview: want 200, got %d (%v)", code, out)
	}
	if !strings.Contains(out["scope"].(string), "tier=wide") {
		t.Errorf("scope should be tier=wide, got %q", out["scope"])
	}
	if n := int(out["will_delete"].(float64)); n != 4 {
		t.Errorf("will_delete: want 4, got %d", n)
	}
	if out["over_cap"] != nil {
		t.Errorf("4 rows is under the default cap, over_cap should be absent: %v", out["over_cap"])
	}

	// Without HYATLAS_ADMIN=on the wide write is refused.
	code, out = deleteCall(t, srv, "", map[string]any{"user_id": "default", "dry_run": false})
	if code != 403 {
		t.Fatalf("wide write without admin: want 403, got %d (%v)", code, out)
	}
	expectRows(t, srv, 4)

	// With the admin switch it is still refused: a wide write needs the mass
	// confirmation on top of it. This is the 10-09 shape exactly.
	srv.admin = true
	code, out = deleteCall(t, srv, "", map[string]any{"user_id": "default", "dry_run": false})
	if code != 428 {
		t.Fatalf("wide write without confirm_mass_delete: want 428, got %d (%v)", code, out)
	}
	if n := int(out["deleted_count"].(float64)); n != 0 {
		t.Errorf("refused wide write deleted %d rows", n)
	}
	expectRows(t, srv, 4)

	// Only the explicit combination deletes.
	code, out = deleteCall(t, srv, "", map[string]any{
		"user_id": "default", "dry_run": false, "confirm_mass_delete": true,
	})
	if code != 200 {
		t.Fatalf("confirmed wide write: want 200, got %d (%v)", code, out)
	}
	if n := int(out["deleted_count"].(float64)); n != 4 {
		t.Errorf("deleted_count: want 4, got %d", n)
	}
	expectRows(t, srv, 0)
}

func TestDeleteAllAgentIDAloneIsWideToo(t *testing.T) {
	srv := newTestServer(t, "m", "http://127.0.0.1:1/v1")
	seedStore(t, srv, memory.L2Raw, "u1", "agent-only", 2)
	srv.admin = true
	code, out := deleteCall(t, srv, "", map[string]any{"agent_id": "agent-only", "dry_run": false})
	if code != 428 {
		t.Fatalf("agent_id alone should be wide, want 428, got %d (%v)", code, out)
	}
	if !strings.Contains(out["scope"].(string), "tier=wide") {
		t.Errorf("scope: %q", out["scope"])
	}
	expectRows(t, srv, 2)
}

// --- Gate 4: the whole-store wipe keeps its own gate ----------------------------

func TestDeleteAllWipeNeedsAdmin(t *testing.T) {
	srv := newTestServer(t, "m", "http://127.0.0.1:1/v1")
	seedStore(t, srv, memory.L3Fact, "alice", "a1", 3)

	cases := []struct {
		name string
		body map[string]any
	}{
		{"no scope at all", map[string]any{}},
		{"all=true", map[string]any{"all": true}},
		{"confirm=wipe-all", map[string]any{"confirm": "wipe-all"}},
		{"layer=*", map[string]any{"layer": "*"}},
	}
	for _, c := range cases {
		code, out := deleteCall(t, srv, "", c.body)
		if code != 403 {
			t.Errorf("%s without admin: want 403, got %d (%v)", c.name, code, out)
		}
	}
	expectRows(t, srv, 3)

	// With the admin switch a wipe is permitted, but still a dry run first.
	srv.admin = true
	code, out := deleteCall(t, srv, "", map[string]any{"all": true})
	if code != 200 || out["dry_run"] != true {
		t.Errorf("wipe preview: got %d %v", code, out)
	}
	if n := int(out["deleted_count"].(float64)); n != 0 {
		t.Errorf("wipe preview deleted %d rows", n)
	}
	expectRows(t, srv, 3)
}

func TestDeleteAllMethodGateUnchanged(t *testing.T) {
	srv := newTestServer(t, "m", "http://127.0.0.1:1/v1")
	ids := seedStore(t, srv, memory.L3Fact, "alice", "a1", 1)
	req := httptest.NewRequest(http.MethodGet, "/api/v1/delete_all", nil)
	req.Host = "127.0.0.1:19528"
	w := httptest.NewRecorder()
	srv.handleDelete(w, req)
	if w.Code != http.StatusMethodNotAllowed {
		t.Errorf("GET: want 405, got %d", w.Code)
	}
	req = httptest.NewRequest(http.MethodDelete, "/api/v1/delete_all", strings.NewReader("{}"))
	req.Host = "127.0.0.1:19528"
	w = httptest.NewRecorder()
	srv.handleDelete(w, req)
	if w.Code != 403 {
		t.Errorf("bare DELETE refused by the unscoped guard, want 403, got %d", w.Code)
	}
	// DELETE with an explicit id is still an accepted, working verb.
	req = httptest.NewRequest(http.MethodDelete, "/api/v1/delete_all?id="+ids[0],
		strings.NewReader("{}"))
	req.Host = "127.0.0.1:19528"
	w = httptest.NewRecorder()
	srv.handleDelete(w, req)
	if w.Code != 200 {
		t.Errorf("DELETE by id: want 200, got %d", w.Code)
	}
}

// --- Plugin compatibility: memory_forget must keep working ----------------------

func TestDeleteAllNarrowIDDeleteStillWorks(t *testing.T) {
	srv := newTestServer(t, "m", "http://127.0.0.1:1/v1")
	ids := seedStore(t, srv, memory.L3Fact, "alice", "a1", 4)
	// No admin, no confirm, no mass flag: exactly what the plugin sends.
	code, out := deleteCall(t, srv, "id="+url.QueryEscape(ids[0]), map[string]any{"dry_run": false})
	if code != 200 {
		t.Fatalf("single id delete: want 200, got %d (%v)", code, out)
	}
	if n := int(out["deleted_count"].(float64)); n != 1 {
		t.Errorf("deleted_count: want 1, got %d", n)
	}
	if err, ok := out["error"]; ok && err != nil {
		t.Errorf("single id delete should not be refused: %v", err)
	}
	expectRows(t, srv, 3)
}

func TestDeleteAllRepeatedIDQueryParams(t *testing.T) {
	srv := newTestServer(t, "m", "http://127.0.0.1:1/v1")
	ids := seedStore(t, srv, memory.L3Fact, "alice", "a1", 5)
	qs := ""
	for _, id := range ids[:3] {
		qs += "id=" + url.QueryEscape(id) + "&"
	}
	code, out := deleteCall(t, srv, qs, map[string]any{"dry_run": false})
	if code != 200 {
		t.Fatalf("repeated id=: want 200, got %d (%v)", code, out)
	}
	if n := int(out["deleted_count"].(float64)); n != 3 {
		t.Errorf("deleted_count: want 3, got %d", n)
	}
	expectRows(t, srv, 2)
}

func TestDeleteAllCommaJoinedIDList(t *testing.T) {
	srv := newTestServer(t, "m", "http://127.0.0.1:1/v1")
	ids := seedStore(t, srv, memory.L3Fact, "alice", "a1", 5)
	qs := "id=" + url.QueryEscape(ids[0]+","+ids[1]+","+ids[2])
	code, out := deleteCall(t, srv, qs, map[string]any{"dry_run": false})
	if code != 200 {
		t.Fatalf("comma-joined ids: want 200, got %d (%v)", code, out)
	}
	if n := int(out["deleted_count"].(float64)); n != 3 {
		t.Errorf("deleted_count: want 3, got %d", n)
	}
	expectRows(t, srv, 2)
}

func TestDeleteAllPluginBatchShapeUnderCap(t *testing.T) {
	srv := newTestServer(t, "m", "http://127.0.0.1:1/v1")
	ids := seedStore(t, srv, memory.L3Fact, "alice", "a1", 120)
	// The plugin's own batch size is 100 ids per request, and its own delete_all
	// refuses to enumerate the whole store. That contract must survive.
	qs := "id=" + url.QueryEscape(strings.Join(ids[:100], ","))
	code, out := deleteCall(t, srv, qs, map[string]any{"dry_run": false})
	if code != 200 {
		t.Fatalf("plugin-shaped 100-id batch: want 200, got %d (%v)", code, out)
	}
	if n := int(out["deleted_count"].(float64)); n != 100 {
		t.Errorf("deleted_count: want 100, got %d", n)
	}
	expectRows(t, srv, 20)
}

func TestDeleteAllNarrowBatchReportsMissingIDs(t *testing.T) {
	srv := newTestServer(t, "m", "http://127.0.0.1:1/v1")
	ids := seedStore(t, srv, memory.L3Fact, "alice", "a1", 2)
	qs := "id=" + url.QueryEscape(ids[0]+",no-such-id,also-missing")
	code, out := deleteCall(t, srv, qs, map[string]any{"dry_run": false})
	if code != 200 {
		t.Fatalf("batch with a missing id: want 200, got %d (%v)", code, out)
	}
	if n := int(out["deleted_count"].(float64)); n != 1 {
		t.Errorf("deleted_count: want 1, got %d", n)
	}
	missing, _ := out["not_found"].(string)
	if !strings.Contains(missing, "no-such-id") || !strings.Contains(missing, "also-missing") {
		t.Errorf("not_found should name the unmatched ids, got %q", missing)
	}
	// Not-found ids are reported, not refused: an idempotent forget must stay
	// re-runnable, so the matched rows are still deleted.
	expectRows(t, srv, 1)
}

func TestDeleteAllPluginEnumerateThenDeleteFlow(t *testing.T) {
	srv := newTestServer(t, "m", "http://127.0.0.1:1/v1")
	seedStore(t, srv, memory.L2Raw, "alice", "a1", 8)
	// plugins/hy_memory/client.py lists the scope, then deletes by id batches.
	// Neither step needs admin, and neither hits the mass gate.
	rows, _ := srv.store.List(memory.L2Raw, "alice", "a1", 100, 0, false)
	if len(rows) != 8 {
		t.Fatalf("seeded %d rows", len(rows))
	}
	idList := make([]string, len(rows))
	for i, d := range rows {
		idList[i] = d.ID
	}
	code, out := deleteCall(t, srv, "id="+url.QueryEscape(strings.Join(idList, ",")),
		map[string]any{"dry_run": false})
	if code != 200 {
		t.Fatalf("plugin flow: want 200, got %d (%v)", code, out)
	}
	if n := int(out["deleted_count"].(float64)); n != 8 {
		t.Errorf("plugin flow deleted_count: want 8, got %d", n)
	}
	expectRows(t, srv, 0)
}

// --- The store's preview count must equal the deletion count ---------------------

func TestDeleteMatchCountEqualsActualDelete(t *testing.T) {
	srv := newTestServer(t, "m", "http://127.0.0.1:1/v1")
	facts := seedStore(t, srv, memory.L3Fact, "alice", "a1", 5)
	raw := seedStore(t, srv, memory.L2Raw, "alice", "a1", 3)
	// One superseded row: Delete targets it, so the preview must too.
	survivor := newID()
	if err := srv.store.Add(memory.L3Fact, survivor, "survivor", map[string]string{"user_id": "alice", "agent_id": "a1"}); err != nil {
		t.Fatal(err)
	}
	if _, err := srv.store.Supersede([]string{facts[0]}, survivor); err != nil {
		t.Fatal(err)
	}

	if n := srv.store.DeleteMatchCount(facts, memory.Layer(""), "alice", "a1"); n != 5 {
		t.Errorf("DeleteMatchCount(facts): want 5 incl. the superseded row, got %d", n)
	}
	if n := srv.store.DeleteMatchCount(nil, memory.Layer(""), "alice", "a1"); n != 9 {
		t.Errorf("DeleteMatchCount(scope): want 9 (5 facts + survivor + 3 raw), got %d", n)
	}
	if n := srv.store.DeleteMatchCount(nil, memory.L2Raw, "alice", "a1"); n != 3 {
		t.Errorf("DeleteMatchCount(l2_raw): want 3, got %d", n)
	}
	if n := srv.store.DeleteMatchCount(raw, memory.Layer(""), "", ""); n != 3 {
		t.Errorf("DeleteMatchCount(raw ids): want 3, got %d", n)
	}
	if n := srv.store.DeleteMatchCount([]string{facts[0], "no-such-id"}, memory.Layer(""), "", ""); n != 1 {
		t.Errorf("DeleteMatchCount(missing id): want 1, got %d", n)
	}

	_, out := deleteCall(t, srv, "", map[string]any{"user_id": "alice", "agent_id": "a1"})
	if n := int(out["will_delete"].(float64)); n != 9 {
		t.Errorf("will_delete: want 9, got %d", n)
	}
	code, out := deleteCall(t, srv, "", map[string]any{"user_id": "alice", "agent_id": "a1", "dry_run": false})
	if code != 200 {
		t.Fatalf("scoped delete: want 200, got %d (%v)", code, out)
	}
	if n := int(out["deleted_count"].(float64)); n != 9 {
		t.Errorf("deleted_count %d != will_delete 9", n)
	}
}

// --- Tier classification ---------------------------------------------------------

func TestClassifyTier(t *testing.T) {
	cases := []struct {
		name string
		sc   deleteScope
		want deleteTier
	}{
		{"no scope at all", deleteScope{}, tierWipe},
		{"all=true", deleteScope{All: true}, tierWipe},
		{"explicit ids", deleteScope{IDs: []string{"a", "b"}}, tierNarrow},
		{"ids win over scope", deleteScope{IDs: []string{"a"}, UserID: "u"}, tierNarrow},
		{"user_id alone", deleteScope{UserID: "default"}, tierWide},
		{"agent_id alone", deleteScope{AgentID: "a1"}, tierWide},
		{"layer alone", deleteScope{Layer: "l3_fact"}, tierScoped},
		{"user_id and agent_id", deleteScope{UserID: "u", AgentID: "a"}, tierScoped},
		{"layer plus both owners", deleteScope{Layer: "l3_fact", UserID: "u", AgentID: "a"}, tierScoped},
	}
	for _, c := range cases {
		if got := classifyTier(c.sc); got != c.want {
			t.Errorf("%s: tier = %s, want %s", c.name, got, c.want)
		}
	}
}

// --- Audit ------------------------------------------------------------------------

func TestDescribeDeleteScopeDoesNotDumpIDs(t *testing.T) {
	ids := []string{}
	for i := 0; i < 100; i++ {
		ids = append(ids, "id"+string(rune('a'+i%26))+string(rune('0'+i/26)))
	}
	desc := describeDeleteScope(deleteScope{IDs: ids}, classifyTier(deleteScope{IDs: ids}))
	if len(desc) > 64 {
		t.Errorf("audit description for 100 ids is %d bytes, should stay short: %q", len(desc), desc)
	}
	if !strings.Contains(desc, "ids=100") {
		t.Errorf("should count ids rather than list them: %q", desc)
	}
	single := describeDeleteScope(deleteScope{IDs: []string{"only-one"}},
		classifyTier(deleteScope{IDs: []string{"only-one"}}))
	if !strings.Contains(single, "id=only-one") {
		t.Errorf("a single id should be named: %q", single)
	}
}

func TestSanitizeAuditField(t *testing.T) {
	if got := sanitizeAuditField("bot\nfaked log line\rmore"); strings.ContainsAny(got, "\n\r") {
		t.Errorf("control chars survived sanitisation: %q", got)
	}
	if long := sanitizeAuditField(strings.Repeat("a", 400)); len(long) > 170 {
		t.Errorf("long user agent not capped: %d bytes", len(long))
	}
}

func TestAuditRemote(t *testing.T) {
	if got := auditRemote("127.0.0.1:54321"); got != "127.0.0.1:54321" {
		t.Errorf("loopback addr: got %q", got)
	}
	if got := auditRemote("[::1]:41000"); got != "[::1]:41000" {
		t.Errorf("ipv6 addr: got %q", got)
	}
	if got := auditRemote("host.without.port"); got != "host.without.port" {
		t.Errorf("no port: got %q", got)
	}
}
