package main

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/tuancookiez-hub/hyatlas-v4/memory"
)

func TestValidUntilExpired(t *testing.T) {
	past := time.Now().Add(-time.Hour).UTC().Format(time.RFC3339)
	future := time.Now().Add(time.Hour).UTC().Format(time.RFC3339)
	if !validUntilExpired(past) {
		t.Error("past timestamp should be expired")
	}
	if validUntilExpired(future) {
		t.Error("future timestamp should not be expired")
	}
	if validUntilExpired("") {
		t.Error("empty = no expiry")
	}
	if validUntilExpired("not-a-date") {
		t.Error("unparsable must fail open")
	}
	if validUntilExpired(past + "!force") {
		t.Error("forced valid_until must never filter (locked content stays visible)")
	}
}

func TestSearchFiltersExpired(t *testing.T) {
	srv := newTestServer(t, "test", "test")
	expired := time.Now().Add(-24 * time.Hour).UTC().Format(time.RFC3339)
	id1, _ := addDoc(srv.store, memory.L2Raw, "记忆优化探针-甲-可召回", map[string]string{})
	id2, _ := addDoc(srv.store, memory.L2Raw, "记忆优化探针-乙-已过期", map[string]string{"valid_until": expired})
	if id1 == "" || id2 == "" {
		t.Fatal("probe add failed")
	}

	// default: expired filtered out
	res, err := srv.store.Search("记忆优化探针", 10, "", "", "", false)
	if err != nil {
		t.Fatal(err)
	}
	if hasID(res, id2) {
		t.Error("expired doc leaked into default search")
	}
	if !hasID(res, id1) {
		t.Error("live doc missing from search")
	}
	// include_expired: both visible
	res, err = srv.store.Search("记忆优化探针", 10, "", "", "", true)
	if err != nil {
		t.Fatal(err)
	}
	if !hasID(res, id2) {
		t.Error("include_expired=true must return expired doc")
	}
}

func TestTouchIDs(t *testing.T) {
	srv := newTestServer(t, "test", "test")
	id, _ := addDoc(srv.store, memory.L3Fact, "touch 测试文档", map[string]string{})
	n, err := srv.store.TouchIDs([]string{id, "no-such-id"})
	if err != nil {
		t.Fatal(err)
	}
	if n != 1 {
		t.Fatalf("touched: want 1, got %d", n)
	}
	d, ok := srv.store.GetDoc(id)
	if !ok {
		t.Fatal("doc vanished")
	}
	if d.Meta["hit_count"] != "1" {
		t.Errorf("hit_count: want 1, got %q", d.Meta["hit_count"])
	}
	if _, err := time.Parse(time.RFC3339, d.Meta["last_hit_at"]); err != nil {
		t.Errorf("last_hit_at not RFC3339: %q", d.Meta["last_hit_at"])
	}
	// second touch increments
	_, _ = srv.store.TouchIDs([]string{id})
	d, _ = srv.store.GetDoc(id)
	if d.Meta["hit_count"] != "2" {
		t.Errorf("hit_count: want 2, got %q", d.Meta["hit_count"])
	}
}

func TestTouchSurvivesRestart(t *testing.T) {
	// True-source test: fields must live in chromem, not only doc_index.json.
	dir := t.TempDir()
	em := NewLocalEmbedder(384)
	s1, err := NewMemoryStore(ctxForTest(), dir, em, filepath.Join(dir, "graph.json"), 384)
	if err != nil {
		t.Skipf("MemoryStore unavailable in test env: %v", err)
	}
	id, _ := addDoc(s1, memory.L3Fact, "真源验证文档", map[string]string{})
	if _, _, err := s1.PatchMeta(id, map[string]string{"superseded_by": "mem-new-1", "valid_until": "2999-01-01T00:00:00Z"}, nil); err != nil {
		t.Fatal(err)
	}
	if _, err := s1.TouchIDs([]string{id}); err != nil {
		t.Fatal(err)
	}
	s1.Close()

	s2, err := NewMemoryStore(ctxForTest(), dir, em, filepath.Join(dir, "graph.json"), 384)
	if err != nil {
		t.Fatal(err)
	}
	defer s2.Close()
	d, ok := s2.GetDoc(id)
	if !ok {
		t.Fatal("doc missing after reopen")
	}
	if d.Meta["superseded_by"] != "mem-new-1" || d.Meta["valid_until"] != "2999-01-01T00:00:00Z" || d.Meta["hit_count"] != "1" {
		t.Errorf("fields lost across restart: %+v", d.Meta)
	}
}

func TestPatchMeta(t *testing.T) {
	srv := newTestServer(t, "test", "test")
	id, _ := addDoc(srv.store, memory.L3Fact, "patch 测试文档", map[string]string{"pin": "old"})
	// set + clear in one shot
	found, old, err := srv.store.PatchMeta(id, map[string]string{"pin": "manual", "ttl_until": "2999-01-01T00:00:00Z"}, []string{"nothing"})
	if err != nil || !found {
		t.Fatalf("patch: found=%v err=%v", found, err)
	}
	if old["pin"] != "old" {
		t.Errorf("previous value: want old, got %q", old["pin"])
	}
	d, _ := srv.store.GetDoc(id)
	if d.Meta["pin"] != "manual" || d.Meta["ttl_until"] != "2999-01-01T00:00:00Z" {
		t.Errorf("patch not mirrored to index: %+v", d.Meta)
	}
	// immutable keys are refused silently (index projection safety)
	if _, _, err := srv.store.PatchMeta(id, map[string]string{"layer": "l1_profile", "ts": "2000-01-01T00:00:00Z"}, nil); err != nil {
		t.Fatal(err)
	}
	d, _ = srv.store.GetDoc(id)
	if d.Layer != string(memory.L3Fact) {
		t.Error("immutable key 'layer' was modified")
	}
	// unknown id -> not found
	if found, _, err := srv.store.PatchMeta("ghost-id", map[string]string{"a": "b"}, nil); found || err == nil {
		t.Error("ghost patch must report not-found")
	}
}

func TestTouchHandler(t *testing.T) {
	srv := newTestServer(t, "test", "test")
	id, _ := addDoc(srv.store, memory.L3Fact, "handler touch", map[string]string{})
	body, _ := json.Marshal(map[string]any{"ids": []string{id}, "source": "search"})
	req := httptest.NewRequest("POST", "/api/v1/touch", bytes.NewReader(body))
	w := httptest.NewRecorder()
	srv.handleTouch(w, req)
	if w.Code != http.StatusOK {
		t.Fatalf("code: want 200, got %d (%s)", w.Code, w.Body.String())
	}
	var out map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &out); err != nil {
		t.Fatal(err)
	}
	if out["touched"].(float64) != 1 {
		t.Errorf("touched: want 1, got %v", out["touched"])
	}
}

func TestPatchHandler(t *testing.T) {
	srv := newTestServer(t, "test", "test")
	id, _ := addDoc(srv.store, memory.L3Fact, "handler patch", map[string]string{})
	post := func(t *testing.T, query string, payload map[string]any) *httptest.ResponseRecorder {
		t.Helper()
		b, _ := json.Marshal(payload)
		req := httptest.NewRequest("POST", "/api/v1/patch"+query, bytes.NewReader(b))
		w := httptest.NewRecorder()
		srv.handlePatch(w, req)
		return w
	}
	// valid RFC3339 set
	if w := post(t, "", map[string]any{"id": id, "set": map[string]string{"valid_until": "2999-01-01T00:00:00Z"}}); w.Code != 200 {
		t.Fatalf("valid patch rejected: %d %s", w.Code, w.Body.String())
	}
	// unparsable date -> 400
	if w := post(t, "", map[string]any{"id": id, "set": map[string]string{"valid_until": "1234567890"}}); w.Code != 400 {
		t.Errorf("epoch value accepted: want 400, got %d", w.Code)
	}
	// past valid_until without force -> 400 (would bury the doc invisibly)
	past := time.Now().Add(-48 * time.Hour).UTC().Format(time.RFC3339)
	if w := post(t, "", map[string]any{"id": id, "set": map[string]string{"valid_until": past}}); w.Code != 400 {
		t.Errorf("pre-gmt valid_until accepted without force: want 400, got %d", w.Code)
	}
	// same with force -> 200 and the stored value carries the suffix
	if w := post(t, "?force=1", map[string]any{"id": id, "set": map[string]string{"valid_until": past}}); w.Code != 200 {
		t.Fatalf("forced patch rejected: %d %s", w.Code, w.Body.String())
	}
	d, _ := srv.store.GetDoc(id)
	if !strings.HasSuffix(d.Meta["valid_until"], "!force") {
		t.Errorf("force suffix missing: %q", d.Meta["valid_until"])
	}
	// unknown id -> 404
	if w := post(t, "", map[string]any{"id": "ghost", "set": map[string]string{"a": "b"}}); w.Code != 404 {
		t.Errorf("ghost patch: want 404, got %d", w.Code)
	}
}

func TestTouchCandidatesGate(t *testing.T) {
	t.Setenv("HYATLAS_TOUCH_MIN_SCORE", "0.72")
	mk := func(score float32) SearchHit {
		return SearchHit{ID: "x", Score: score, Layer: memory.L2Raw}
	}
	// top-3 + score gate; rank 4 never touched even if above gate
	res := []SearchHit{mk(0.9), mk(0.71), mk(0.8), mk(0.95)}
	ids := touchCandidates(res)
	if len(ids) != 2 { // 0.9 and 0.8; 0.71 below gate; 4th beyond rank 3
		t.Fatalf("want 2 gated ids, got %v", ids)
	}
	// gate <= 0 disables entirely (fallback switch)
	t.Setenv("HYATLAS_TOUCH_MIN_SCORE", "0")
	if ids := touchCandidates([]SearchHit{mk(0.99)}); len(ids) != 0 {
		t.Errorf("gate=0 must disable touch, got %v", ids)
	}
}

// helpers

func addDoc(store *MemoryStore, layer memory.Layer, content string, meta map[string]string) (string, error) {
	id := newID()
	m := map[string]string{"user_id": "default", "agent_id": "default",
		"ts": time.Now().UTC().Format(time.RFC3339)}
	for k, v := range meta {
		m[k] = v
	}
	return id, store.Add(layer, id, content, m)
}

func hasID(hits []SearchHit, id string) bool {
	for _, h := range hits {
		if h.ID == id {
			return true
		}
	}
	return false
}
