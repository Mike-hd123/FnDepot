package main

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"testing"
)

func newTestServer(t *testing.T, model, base string) *Server {
	t.Helper()
	dir := t.TempDir()
	em := NewLocalEmbedder(384)
	store, err := NewMemoryStore(ctxForTest(), dir, em, filepath.Join(dir, "graph.json"), 384)
	if err != nil {
		t.Skipf("MemoryStore unavailable in test env: %v", err)
	}
	t.Cleanup(func() { store.Close() })
	return &Server{
		store:    store,
		llmModel: model,
		llmBase:  base,
		start:    timeForTest(),
	}
}

func TestHandleStatusReportsLiveLLMModel(t *testing.T) {
	srv := newTestServer(t,
		"poolside/laguna-s-2.1:free",
		"https://inference-api.nousresearch.com/v1")
	req := httptest.NewRequest("GET", "/api/v1/status", nil)
	w := httptest.NewRecorder()
	srv.handleStatus(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("status code: want 200, got %d", w.Code)
	}
	var body map[string]any
	if err := json.NewDecoder(w.Body).Decode(&body); err != nil {
		t.Fatal(err)
	}
	if got, _ := body["llm_model"].(string); got != "poolside/laguna-s-2.1:free" {
		t.Errorf("llm_model: want poolside/laguna-s-2.1:free, got %q", got)
	}
	if got, _ := body["llm_base"].(string); got != "https://inference-api.nousresearch.com/v1" {
		t.Errorf("llm_base: got %q", got)
	}
}

func TestHandleDashInfoReportsLiveLLMModel(t *testing.T) {
	srv := newTestServer(t,
		"inclusionai/ling-3.0-flash-fin:free",
		"https://inference-api.nousresearch.com/v1")
	req := httptest.NewRequest("GET", "/api/info", nil)
	w := httptest.NewRecorder()
	srv.handleDashInfo(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("status code: want 200, got %d", w.Code)
	}
	var body map[string]any
	if err := json.NewDecoder(w.Body).Decode(&body); err != nil {
		t.Fatal(err)
	}
	if got, _ := body["llm_model"].(string); got != "inclusionai/ling-3.0-flash-fin:free" {
		t.Errorf("llm_model: got %q", got)
	}
	if got, _ := body["llm_base"].(string); got != "https://inference-api.nousresearch.com/v1" {
		t.Errorf("llm_base: got %q", got)
	}
}

// TestPromoteExtractionThreadsSource asserts that L5 edges are anchored to the
// L2 memory id passed in. This is the evidence-citation contract.
// System1 (the per-turn pass) owns L3/L4/L7 and must NOT touch L5/L6: those are
// System2 products, synthesised across many memories by the consolidation pass.
// A knowledge relation worth keeping is one corroborated by more than a single
// turn, and a schema is a *recurring* pattern — neither is observable from one
// turn. This test pins both halves of that contract: provenance is threaded onto
// the fact, and no graph edge or schema row is written here.
func TestPromoteExtractionThreadsSource(t *testing.T) {
	srv := newTestServer(t, "test", "test")
	// Even when the model volunteers knowledge and schemas, the per-turn pass must
	// ignore them: the prompt no longer requests them and System2 owns the layers.
	ex, err := parseExtraction(`{"facts":[{"data":"Apple is in Cupertino","layer":"project_state"}],
		"knowledge":[{"from":"Apple","relation":"based_in","to":"Cupertino"}],
		"schemas":[{"pattern":"companies have a HQ","context":"geo facts"}]}`)
	if err != nil {
		t.Fatal(err)
	}
	sourceID := "mem-test-source-abc"
	promoteExtractionCtx(srv.store, ex, ExtractContext{}, "default", "default", sourceID, 0)

	// No L5 edge from the per-turn pass.
	if _, rels := srv.store.Graph().Snapshot(50); len(rels) != 0 {
		t.Errorf("per-turn pass wrote %d L5 edge(s); L5 belongs to the slow path", len(rels))
	}
	// No L6 schema row either.
	if _, n := srv.store.List("l6_schema", "", "", 1, 0, false); n != 0 {
		t.Errorf("per-turn pass wrote %d L6 schema(s); L6 belongs to the slow path", n)
	}

	// The L3 fact must carry its L2 source so the slow path can cite it and raw
	// decay can protect the row a live claim still depends on.
	_, total := srv.store.List("l3_fact", "", "", 1, 0, false)
	rows, _ := srv.store.List("l3_fact", "", "", total, 0, false)
	var found bool
	for _, d := range rows {
		if d.Content != "Apple is in Cupertino" {
			continue
		}
		found = true
		if d.Meta["source_id"] != sourceID {
			t.Errorf("L3 source_id = %q, want %q (provenance lost)", d.Meta["source_id"], sourceID)
		}
	}
	if !found {
		t.Error("the L3 fact was not stored")
	}
}

// System1 extractor must not write L1, and (as of the 4.5.0 sync) it also no
// longer writes a second "twin" L3 row. The source_kind=agent_extract duplicate
// was removed for two reasons: (a) it broke 4.5.0 turn-sized consolidation — a
// twin without source_id looks like its own observation to consolidate.go, so a
// single turn inflated to two "distinct" ones; (b) it defeated write-time
// dedupe, which supersedes only one nearest fact, so a twin survived every
// restatement as a permanent near-duplicate — the noise 4.5.0 was built to
// remove. See server.go promoteExtractionCtx for the full rationale, and
// hy450-sync/merge-execution.md for the historical-twin cleanup path.
//
// L1 is writable only through explicit /add or /patch, so it stays an
// agent-curated identity channel; adjudication lives agent-side in the plugin.
// The System2 consolidate pass still mirrors merged user_preferences into L1
// (consolidate.go), so a merged preference reaches the profile view — just not
// every single extracted one.
//
// This test replaces upstream v4.5.0's
// TestPromoteExtractionMirrorsPreferencesToProfile, which asserted the L1
// mirror fork deliberately removed (plan v2 §3④': the mirror was the only
// automated write path into L1 and the source of profile contradictions from
// untrusted external content).
//
// DELIBERATE BEHAVIOUR CHANGE — not an assertion loosened to make a test pass.
// L1 stays 0 (fork core intent, unchanged); L3 goes from 2 to 1 (twin removed).
func TestPromoteExtractionKeepsPreferencesOutOfProfile(t *testing.T) {
	srv := newTestServer(t, "test", "test")
	sourceID := "mem-pref-source"
	promoteExtractionCtx(srv.store, &Extraction{
		Facts: []Fact{{Data: "prefers tabs over spaces", Layer: "user_preferences"}},
	}, ExtractContext{}, "default", "default", sourceID, 0)

	if _, total := srv.store.List("l1_profile", "", "", 1, 0, false); total != 0 {
		t.Fatalf("L1 profile count = %d, want 0 (extractor must not write L1)", total)
	}
	rows, total := srv.store.List("l3_fact", "", "", 10, 0, false)
	if total != 1 {
		t.Fatalf("L3 fact count = %d, want 1 (single row, no agent_extract twin)", total)
	}
	r := rows[0]
	if r.Meta["source_id"] != sourceID {
		t.Errorf("source_id = %q, want %q (twin-less row must keep provenance)", r.Meta["source_id"], sourceID)
	}
	if r.Meta["source_layer_label"] != "user_preferences" {
		t.Errorf("source_layer_label = %q, want user_preferences", r.Meta["source_layer_label"])
	}
	if r.Meta["source_kind"] == "agent_extract" {
		t.Errorf("source_kind = %q, want empty (twin must not be written)", r.Meta["source_kind"])
	}
}

// The intention is a System1 product and is written; a per-write summary is not,
// because episodes (episode.go) summarise whole stretches of work instead.
func TestPromoteExtractionWritesIntentionNotSummary(t *testing.T) {
	srv := newTestServer(t, "test", "test")
	ex, err := parseExtraction(`{"summary":{"text":"configured the service port"},"intention":{"goal":"ship the listener"}}`)
	if err != nil {
		t.Fatal(err)
	}
	promoteExtractionCtx(srv.store, ex, ExtractContext{}, "default", "default", "mem-si-source", 0)

	if _, n := srv.store.List("l4_summary", "", "", 1, 0, false); n != 0 {
		t.Errorf("L4 summary count = %d, want 0", n)
	}
	if _, n := srv.store.List("l7_intention", "", "", 1, 0, false); n != 1 {
		t.Errorf("L7 intention count = %d, want 1", n)
	}
}
