package main

import (
	"bytes"
	"encoding/json"
	"net/http/httptest"
	"strconv"
	"testing"
	"time"

	"github.com/tuancookiez-hub/hyatlas-v4/memory"
)

// P1.5 end-to-end: a supersede patch on an L2 doc must close the graph edges
// citing it (ValidTo/InvalidatedAt written — the fields that had zero writers
// since day one) and /graph-as-of must then show the edge before the close
// moment and hide it after. Acceptance for kanban t_9de5510c.
func TestPatchSupersedeClosesGraphEdgesAndAsOf(t *testing.T) {
	srv := newTestServer(t, "test", "test")
	// newTestServer silently skips when the local embedder is unavailable in
	// the test env; guard so the P1.5 e2e is never a false PASS on an empty srv.
	if srv == nil || srv.store == nil {
		t.Skip("MemoryStore unavailable in test env")
	}

	srcID, err := addDoc(srv.store, memory.L2Raw, "用户说住在岑村", map[string]string{})
	if err != nil {
		t.Fatal(err)
	}
	if err := srv.store.Graph().AddEdgeWithSource("用户", "lives_in", "岑村", srcID); err != nil {
		t.Fatal(err)
	}
	// The edge records RecordedAt/ValidFrom = now (second resolution). Wait a
	// beat so the supersede's ValidTo lands on a later second and the
	// before-close as-of probe (anchor-1) sits inside the edge's validity
	// window rather than before its recording.
	time.Sleep(1100 * time.Millisecond)

	// patch = supersede (the adjudicator writes exactly these two keys —
	// plugins/hy_memory/adjudicate.py _patch_supersede)
	patchBody, _ := json.Marshal(map[string]any{
		"id": srcID,
		"set": map[string]string{
			"superseded_by": "mem-replacement-1",
			"valid_until":   "2999-01-01T00:00:00Z",
		},
	})
	w := httptest.NewRecorder()
	srv.handlePatch(w, httptest.NewRequest("POST", "/api/v1/patch", bytes.NewReader(patchBody)))
	if w.Code != 200 {
		t.Fatalf("patch code: %d %s", w.Code, w.Body.String())
	}
	var pr struct {
		GraphEdgesClosed int `json:"graph_edges_closed"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &pr); err != nil {
		t.Fatal(err)
	}
	if pr.GraphEdgesClosed != 1 {
		t.Fatalf("patch response must report the close: got %d", pr.GraphEdgesClosed)
	}

	// Read the real anchor off the edge rather than guessing from the wall
	// clock — SnapshotAsOf uses ValidTo <= t, so a probe that lands on the
	// same second as the close would already hide the edge.
	var anchor int64
	_, rels := srv.store.Graph().Snapshot(10)
	for _, e := range rels {
		if e.Relation == "lives_in" {
			anchor = e.ValidTo
		}
	}
	if anchor == 0 {
		t.Fatal("edge has no ValidTo after supersede patch")
	}

	// /asof just before the close still sees the edge...
	w = httptest.NewRecorder()
	srv.handleGraphAsOf(w, httptest.NewRequest("GET", "/api/v1/graph-as-of?ts="+itoa(anchor-1), nil))
	if !hasRelation(w, "lives_in") {
		t.Fatalf("edge must be visible before valid_to, body=%s", w.Body.String())
	}
	// ...and at/after it is gone (this is what "restore the old state" proves).
	w = httptest.NewRecorder()
	srv.handleGraphAsOf(w, httptest.NewRequest("GET", "/api/v1/graph-as-of?ts="+itoa(anchor+1), nil))
	if hasRelation(w, "lives_in") {
		t.Fatalf("closed edge must disappear after valid_to, body=%s", w.Body.String())
	}
}

// A patch that is NOT a supersede must leave the graph untouched, and a second
// supersede of the same doc must not re-report closes (monotonic).
func TestPatchNonSupersedeLeavesGraphOpen(t *testing.T) {
	srv := newTestServer(t, "test", "test")
	srcID, _ := addDoc(srv.store, memory.L2Raw, "普通原始记忆", map[string]string{})
	if err := srv.store.Graph().AddEdgeWithSource("猫", "likes", "罐头", srcID); err != nil {
		t.Fatal(err)
	}

	call := func(set map[string]string) map[string]any {
		b, _ := json.Marshal(map[string]any{"id": srcID, "set": set})
		w := httptest.NewRecorder()
		srv.handlePatch(w, httptest.NewRequest("POST", "/api/v1/patch", bytes.NewReader(b)))
		if w.Code != 200 {
			t.Fatalf("patch %v code %d: %s", set, w.Code, w.Body.String())
		}
		var out map[string]any
		if err := json.Unmarshal(w.Body.Bytes(), &out); err != nil {
			t.Fatal(err)
		}
		return out
	}
	if got := call(map[string]string{"note": "无关字段"}); got["graph_edges_closed"] != float64(0) {
		t.Fatalf("plain field patch must not touch the graph, got %v", got)
	}
	if got := call(map[string]string{"superseded_by": "x", "valid_until": "2999-01-01T00:00:00Z"}); got["graph_edges_closed"] != float64(1) {
		t.Fatalf("first supersede must close 1, got %v", got)
	}
	if got := call(map[string]string{"superseded_by": "y", "valid_until": "2999-06-01T00:00:00Z"}); got["graph_edges_closed"] != float64(0) {
		t.Fatalf("second supersede must be monotonic (0 new closes), got %v", got)
	}
}

func hasRelation(w *httptest.ResponseRecorder, rel string) bool {
	var body struct {
		Relations []struct {
			Relation string `json:"relation"`
		} `json:"relations"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		return false
	}
	for _, r := range body.Relations {
		if r.Relation == rel {
			return true
		}
	}
	return false
}

func itoa(v int64) string {
	return strconv.FormatInt(v, 10)
}
