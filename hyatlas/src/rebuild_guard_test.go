package main

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/tuancookiez-hub/hyatlas-v4/memory"
)

// t_a7ff4ed2 regression tests: rebuildIndex stability hardening.
//   fix 1 — dim-mismatch / embedder failures no longer skip layers silently
//   fix 2 — a broken rebuild must NOT overwrite a good doc_index.json
//   fix 3 — remote-embedder fallback waits for readiness (unit: probe gating)

// wrongDimEmbedder always returns vectors of a fixed (wrong) dimension,
// mimicking "config drifted from 1024d stock data to 384d/96d".
type wrongDimEmbedder struct{ dim int }

func (e wrongDimEmbedder) Embed(_ context.Context, _ string) ([]float32, error) {
	v := make([]float32, e.dim)
	v[0] = 1 // normalized
	return v, nil
}
func (e wrongDimEmbedder) Dim() int { return e.dim }

// deadEmbedder always errors, mimicking "embedder endpoint down".
type deadEmbedder struct{}

func (deadEmbedder) Embed(_ context.Context, _ string) ([]float32, error) {
	return nil, errors.New("embedder down")
}
func (deadEmbedder) Dim() int { return 0 }

func countIndexOnDisk(t *testing.T, path string) int {
	t.Helper()
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read doc_index.json: %v", err)
	}
	var m map[string]json.RawMessage
	if err := json.Unmarshal(b, &m); err != nil {
		t.Fatalf("parse doc_index.json: %v", err)
	}
	return len(m)
}

// A store restarted with a mismatched embedder dimension must fail every
// layer query (zero-vector length mismatch + fallback embed mismatch) and
// REFUSE to persist — the previous good index stays on disk untouched.
func TestRebuildIndexRefusesOverwriteOnDimMismatch(t *testing.T) {
	dir := t.TempDir()
	index := filepath.Join(dir, "doc_index.json")

	// healthy generation: 1024d, a few docs across layers
	s1, err := NewMemoryStore(ctxForTest(), dir, NewLocalEmbedder(1024), filepath.Join(dir, "graph.json"), 1024)
	if err != nil {
		t.Skipf("MemoryStore unavailable in test env: %v", err)
	}
	for _, l := range []memory.Layer{memory.L2Raw, memory.L3Fact, memory.L7Intention} {
		if _, err := addDoc(s1, l, "内容 "+string(l), map[string]string{}); err != nil {
			t.Fatal(err)
		}
	}
	s1.Close()

	before := countIndexOnDisk(t, index)
	if before < 3 {
		t.Fatalf("expected >=3 indexed docs on disk, got %d", before)
	}

	// broken generation: same store dir, embedder now yields 96d vectors.
	// rebuildIndex zero-vector query (1024d configured) mismatches stored
	// 96d... actually dims param also 1024 here; the mismatch is inside
	// chromem: stored docs are 1024d, QueryEmbedding gets a 1024d zero vec
	// fine — so force the failure via dims=384 like the production incident.
	s2, err := NewMemoryStore(ctxForTest(), dir, wrongDimEmbedder{dim: 96}, filepath.Join(dir, "graph2.json"), 384)
	if err != nil {
		t.Fatalf("reopen with drifted dims: %v", err)
	}
	defer s2.Close()

	if got := s2.TotalMemories(); got >= before/2 {
		t.Errorf("broken rebuild populated the index map (%d entries, previous=%d) — guard ineffective", got, before)
	}
	if after := countIndexOnDisk(t, index); after != before {
		t.Errorf("doc_index.json was overwritten by a broken rebuild: %d -> %d entries", before, after)
	}
}

// When the embedder endpoint is down at startup, rebuild must fail the
// layers loudly (fix 1) and keep the old index (fix 2); the live chromem
// data is untouched and a later healthy restart re-indexes fully.
func TestRebuildIndexKeepsIndexWhenEmbedderDead(t *testing.T) {
	dir := t.TempDir()
	index := filepath.Join(dir, "doc_index.json")

	s1, err := NewMemoryStore(ctxForTest(), dir, NewLocalEmbedder(384), filepath.Join(dir, "graph.json"), 384)
	if err != nil {
		t.Skipf("MemoryStore unavailable in test env: %v", err)
	}
	for i := 0; i < 5; i++ {
		if _, err := addDoc(s1, memory.L3Fact, "事实条目", map[string]string{}); err != nil {
			t.Fatal(err)
		}
	}
	s1.Close()
	before := countIndexOnDisk(t, index)

	// production incident shape: dims config drifted (96 vs stored 384) so the
	// zero-vector enumeration fails AND the remote embedder is down so the
	// text-query fallback fails too → every layer is lost → must refuse persist.
	s2, err := NewMemoryStore(ctxForTest(), dir, deadEmbedder{}, filepath.Join(dir, "graph.json"), 96)
	if err != nil {
		t.Fatalf("reopen with dead embedder: %v", err)
	}
	if after := countIndexOnDisk(t, index); after != before {
		s2.Close()
		t.Errorf("dead embedder rebuild overwrote the index: %d -> %d", before, after)
	}
	s2.Close()

	// healthy restart fully recovers the index (chromem was the source of truth all along)
	s3, err := NewMemoryStore(ctxForTest(), dir, NewLocalEmbedder(384), filepath.Join(dir, "graph.json"), 384)
	if err != nil {
		t.Fatalf("healthy reopen: %v", err)
	}
	defer s3.Close()
	if got := s3.TotalMemories(); got < before {
		t.Errorf("healthy restart did not fully rebuild: %d < %d", got, before)
	}
}

// needsEmbedderProbe gates the readiness wait to remote embedders only —
// embedded/local models must not add startup latency.
func TestNeedsEmbedderProbeGating(t *testing.T) {
	s := &MemoryStore{embed: NewLocalEmbedder(384)}
	if s.needsEmbedderProbe() {
		t.Error("LocalEmbedder must not trigger the remote readiness probe")
	}
	s = &MemoryStore{embed: NewOpenAIEmbedder("http://127.0.0.1:1/v1", "", "m")}
	if !s.needsEmbedderProbe() {
		t.Error("OpenAIEmbedder must trigger the remote readiness probe")
	}
	s = &MemoryStore{embed: deadEmbedder{}}
	if s.needsEmbedderProbe() {
		t.Error("non-remote embedder must not trigger the probe")
	}
}

// persistGuard unit matrix.
func TestPersistGuard(t *testing.T) {
	s := &MemoryStore{indexPath: filepath.Join(t.TempDir(), "missing.json")}
	if r := s.persistGuard(10, 10); r != "" {
		t.Errorf("full rebuild must pass: %q", r)
	}
	if r := s.persistGuard(4, 10); r == "" {
		t.Error("4/10 recovered must be refused")
	}
	if r := s.persistGuard(5, 10); r != "" {
		t.Errorf("exactly 50%% must pass (guard is <50%%): %q", r)
	}
	if r := s.persistGuard(0, 0); r != "" {
		t.Errorf("empty collection (0/0) must persist an empty index: %q", r)
	}
	// previous file larger than chromem expectation catches partial queries
	prev := filepath.Join(t.TempDir(), "doc_index.json")
	if err := os.WriteFile(prev, []byte(`{"a":{},"b":{},"c":{},"d":{},"e":{}}`), 0o644); err != nil {
		t.Fatal(err)
	}
	s2 := &MemoryStore{indexPath: prev}
	if r := s2.persistGuard(2, 4); r == "" {
		t.Error("indexed=2 vs previous file of 5 must be refused even if expected=4 passes")
	}
}
