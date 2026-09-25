package main

import (
	"path/filepath"
	"testing"
	"time"

	"github.com/tuancookiez-hub/hyatlas-v4/memory"
)

// P1.5 §5.2#1: the layer label persisted in meta must always agree with the
// collection prefix, and a nil bag must stay nil (old contract, consumers
// branch on it.Meta != nil).

func TestDocIndexFromHealsLayerLabel(t *testing.T) {
	// missing label -> persisted
	d := docIndexFrom("id1", "l2_raw", "x", map[string]string{"user_id": "u"})
	if d.Meta["layer"] != "l2_raw" {
		t.Errorf("missing label not healed: %+v", d.Meta)
	}
	// contradictory label -> corrected to the prefix ground truth
	d = docIndexFrom("id2", "l2_raw", "x", map[string]string{"layer": "l3_fact"})
	if d.Meta["layer"] != "l2_raw" {
		t.Errorf("wrong label not repaired: %q", d.Meta["layer"])
	}
	// matching label -> untouched
	d = docIndexFrom("id3", "l3_fact", "x", map[string]string{"layer": "l3_fact", "note": "keep"})
	if d.Meta["layer"] != "l3_fact" || d.Meta["note"] != "keep" {
		t.Errorf("matching label mangled: %+v", d.Meta)
	}
	// nil bag -> nil Meta (contract)
	if d := docIndexFrom("id4", "l2_raw", "x", nil); d.Meta != nil {
		t.Errorf("nil bag must stay nil Meta, got %+v", d.Meta)
	}
}

func TestLayerLabelSurvivesRestart(t *testing.T) {
	dir := t.TempDir()
	em := NewLocalEmbedder(384)
	s1, err := NewMemoryStore(ctxForTest(), dir, em, filepath.Join(dir, "graph.json"), 384)
	if err != nil {
		t.Skipf("MemoryStore unavailable in test env: %v", err)
	}
	id, _ := addDoc(s1, memory.L2Raw, "层标签持久化", map[string]string{})
	d, ok := s1.GetDoc(id)
	if !ok {
		t.Fatal("doc missing before restart")
	}
	if d.Meta["layer"] != "l2_raw" {
		t.Fatalf("label missing on fresh write: %+v", d.Meta)
	}
	s1.Close()

	s2, err := NewMemoryStore(ctxForTest(), dir, em, filepath.Join(dir, "graph.json"), 384)
	if err != nil {
		t.Fatal(err)
	}
	defer s2.Close()
	d, ok = s2.GetDoc(id)
	if !ok {
		t.Fatal("doc missing after reopen")
	}
	if d.Meta["layer"] != "l2_raw" || d.Layer != "l2_raw" {
		t.Errorf("label lost across restart: %+v", d)
	}
}

var _ = time.Now // keep import if helpers shift
