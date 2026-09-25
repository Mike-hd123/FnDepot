package graph

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

// TestSupersedeEdgesClosesOnlyOngoingEdges locks down the bitemporal close
// (write) semantics that /asof reconstruction depends on: ValidTo/InvalidatedAt
// advance only on the supersede path, only for edges that are still ongoing,
// and only for the cited source memory.
func TestSupersedeEdgesClosesOnlyOngoingEdges(t *testing.T) {
	dir := t.TempDir()
	s, err := New(filepath.Join(dir, "g.json"))
	if err != nil {
		t.Fatal(err)
	}
	// Two sources; the superseded one owns exactly one edge.
	if err := s.AddEdgeWithSource("alice", "knows", "bob", "mem-victim"); err != nil {
		t.Fatal(err)
	}
	if err := s.AddEdgeWithSource("carol", "owns", "dave", "mem-keeper"); err != nil {
		t.Fatal(err)
	}

	at := time.Now().Unix()
	closed, err := s.SupersedeEdges("mem-victim", at)
	if err != nil {
		t.Fatal(err)
	}
	if closed != 1 {
		t.Fatalf("want 1 closed, got %d", closed)
	}

	_, rels := s.Snapshot(10)
	byRel := map[string]Edge{}
	for _, e := range rels {
		byRel[e.Relation] = e
	}
	v := byRel["knows"]
	if v.ValidTo != at || v.InvalidatedAt != at {
		t.Errorf("victim edge not closed: valid_to=%d invalidated_at=%d want %d",
			v.ValidTo, v.InvalidatedAt, at)
	}
	k := byRel["owns"]
	if k.ValidTo != 0 || k.InvalidatedAt != 0 {
		t.Errorf("sibling edge must stay open: %+v", k)
	}
}

// TestSupersedeEdgesIsMonotonic: the first close wins — a second supersede of
// the same source must not move the anchor, and unknown/blank sources are no-ops.
func TestSupersedeEdgesIsMonotonic(t *testing.T) {
	dir := t.TempDir()
	s, err := New(filepath.Join(dir, "g.json"))
	if err != nil {
		t.Fatal(err)
	}
	if err := s.AddEdgeWithSource("a", "r", "b", "src1"); err != nil {
		t.Fatal(err)
	}

	t1 := time.Now().Unix()
	if closed, _ := s.SupersedeEdges("src1", t1); closed != 1 {
		t.Fatalf("first close want 1, got %d", closed)
	}
	time.Sleep(time.Second)
	if closed, _ := s.SupersedeEdges("src1", time.Now().Unix()); closed != 0 {
		t.Fatalf("second close must be a no-op, got %d", closed)
	}
	_, rels := s.Snapshot(10)
	for _, e := range rels {
		if e.Relation == "r" && e.ValidTo != t1 {
			t.Fatalf("anchor moved: got %d want %d", e.ValidTo, t1)
		}
	}
	if closed, _ := s.SupersedeEdges("", t1); closed != 0 {
		t.Fatalf("blank source must close nothing, got %d", closed)
	}
	if closed, _ := s.SupersedeEdges("no-such-src", t1); closed != 0 {
		t.Fatalf("unknown source must close nothing, got %d", closed)
	}
}

// TestSupersedeEdgesSurvivesRestart proves the close is persisted, so /asof on
// a cold boot still hides the invalidated edge.
func TestSupersedeEdgesSurvivesRestart(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "g.json")
	s, err := New(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := s.AddEdgeWithSource("a", "r", "b", "src"); err != nil {
		t.Fatal(err)
	}
	if _, err := s.SupersedeEdges("src", time.Now().Unix()); err != nil {
		t.Fatal(err)
	}

	if _, err := os.Stat(path); err != nil {
		t.Fatal(err)
	}
	s2, err := New(path)
	if err != nil {
		t.Fatal(err)
	}
	_, rels := s2.Snapshot(10)
	if len(rels) != 1 {
		t.Fatalf("want 1 edge after reload, got %d", len(rels))
	}
	if rels[0].ValidTo == 0 || rels[0].InvalidatedAt == 0 {
		t.Fatalf("close lost on reload: %+v", rels[0])
	}
}
