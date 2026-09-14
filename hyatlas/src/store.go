package main

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"log"
	"sync"
	"sync/atomic"
	"time"

	"github.com/philippgille/chromem-go"
	"github.com/tuancookiez-hub/hyatlas-v4/graph"
	"github.com/tuancookiez-hub/hyatlas-v4/memory"
)

// UsageCounters is the JSON shape persisted next to the doc index.
type UsageCounters struct {
	Writes   uint64 `json:"writes"`
	Searches uint64 `json:"searches"`
}

// DocIndex is an exact-match record for a stored memory doc. It powers list /
// delete / metrics / scoping without relying on approximate vector search.
type DocIndex struct {
	ID        string `json:"id"`
	Layer     string `json:"layer"`
	Content   string `json:"content"`
	UserID    string `json:"user_id"`
	AgentID   string `json:"agent_id"`
	Ts        string `json:"ts"`
	Extracted bool   `json:"extracted"`
	// Meta carries the full metadata bag at index-write time. Lets list
	// endpoints surface session_id, source_layer_label, etc. without a
	// second lookup. May be nil for older index files.
	Meta map[string]string `json:"meta,omitempty"`
}

// MemoryStore holds layer collections (vectors) + a doc index (exact) + the L5 graph.
type MemoryStore struct {
	db    *chromem.DB
	g     *graph.Store
	embed Embedder
	ctx   context.Context
	cols  map[memory.Layer]*chromem.Collection

	mu    sync.RWMutex
	index map[string]DocIndex
	// persisted index path (same dir as the chromem DB)
	indexPath string
	// usage counters — atomic so reads from /api/v1/status never block writes.
	// Persisted as JSON next to the doc index so they survive restart.
	writes     atomic.Uint64
	searches   atomic.Uint64
	countsPath string
	// embed dimension (384 en / 1024 zh), set at open time
	dims       int

	// --- index write coalescing (reduces SSD writes from ~11 GB/day to ~0.5 GB/day) ---
	// Each persistIndex() call marks dirty + schedules a debounced flush. Rapid
	// writes within HYATLAS_INDEX_FLUSH_SEC (default 1s) collapse into ONE disk write.
	// The on-disk index may lag up to that many seconds; crash within that window
	// loses only the in-flight batch (chromem is the source of truth; rebuildIndex
	// restores the exact index on next startup).
	// See: 2026-09-14 SSD write analysis (t_d079e5f5).
	indexFlushMu sync.Mutex
	indexDirty   bool
	indexTimer   *time.Timer
	indexFlushCh chan struct{} // closed once; drains pending flush before Close()
	indexFlushed bool
}

// NewMemoryStore opens (or creates) the persistent layer DB + graph + doc index.
func NewMemoryStore(ctx context.Context, dir string, embed Embedder, graphPath string, dims int) (*MemoryStore, error) {
	db, err := chromem.NewPersistentDB(dir, false)
	if err != nil {
		return nil, err
	}
	g, err := graph.New(graphPath)
	if err != nil {
		return nil, err
	}
	ef := func(c context.Context, text string) ([]float32, error) {
		return embed.Embed(c, text)
	}
	s := &MemoryStore{db: db, g: g, embed: embed, ctx: ctx,
		cols: map[memory.Layer]*chromem.Collection{}, index: map[string]DocIndex{},
		indexPath:  filepath.Join(dir, "doc_index.json"),
		countsPath: filepath.Join(dir, "usage.json"),
		dims:       dims,
		indexFlushCh: make(chan struct{})}
	// load persisted counters before rebuildIndex so writes/searches survive restart.
	s.loadUsage()
	for _, l := range memory.All() {
		col, err := db.GetOrCreateCollection(string(l), nil, ef)
		if err != nil {
			return nil, err
		}
		s.cols[l] = col
	}
	// rebuild the exact index from chromem's persisted docs
	if err := s.rebuildIndex(); err != nil {
		return nil, err
	}
	return s, nil
}

// Add writes a doc into a layer collection and updates the exact index.
func (s *MemoryStore) Add(layer memory.Layer, id, content string, meta map[string]string) error {
	doc := chromem.Document{ID: id, Content: content}
	if meta != nil {
		doc.Metadata = meta
	}
	if doc.Metadata == nil {
		doc.Metadata = map[string]string{}
	}
	doc.Metadata["layer"] = string(layer)
	if err := s.cols[layer].AddDocument(s.ctx, doc); err != nil {
		return err
	}
	s.mu.Lock()
	s.index[id] = docIndexFrom(id, string(layer), content, meta)
	s.mu.Unlock()
	s.writes.Add(1)
	s.persistUsageAsync()
	return s.persistIndex()
}

// Search does vector search, scoped to user/agent when provided.
func (s *MemoryStore) Search(query string, limit int, layer memory.Layer, userID, agentID string) ([]SearchHit, error) {
	if limit <= 0 {
		limit = 5
	}
	layers := []memory.Layer{}
	if layer != "" {
		layers = []memory.Layer{layer}
	} else {
		layers = memory.All()
	}
	where := map[string]string{}
	if userID != "" {
		where["user_id"] = userID
	}
	if agentID != "" {
		where["agent_id"] = agentID
	}
	if len(where) == 0 {
		where = nil
	}

	var hits []SearchHit
	for _, l := range layers {
		col := s.cols[l]
		n := col.Count()
		k := limit // note: chromem requires k <= n; guard below
		if k > n {
			k = n
		}
		if k <= 0 {
			continue
		}
		res, err := col.Query(s.ctx, query, k, where, nil)
		if err != nil {
			return nil, err
		}
		for _, r := range res {
			hits = append(hits, SearchHit{ID: r.ID, Content: r.Content,
				Score: r.Similarity, Layer: memory.Layer(l), Meta: r.Metadata})
		}
	}
	sort.Slice(hits, func(i, j int) bool { return hits[i].Score > hits[j].Score })
	if len(hits) > limit {
		hits = hits[:limit]
	}
	s.searches.Add(1)
	s.persistUsageAsync()
	return hits, nil
}

// SearchHit is a ranked result tagged with its layer.
type SearchHit struct {
	ID      string
	Content string
	Score   float32
	Layer   memory.Layer
	Meta    map[string]string
}

// TopL7Similar returns the best similarity score for goal text within the L7
// intention collection (top-N match, N capped at min(N, doc count), scoped to
// user/agent when provided). Returns (score, found): found=false when the
// layer is empty, the scope matches nothing, or the query fails — callers
// treat that as "no duplicate" and write (fail-open, matching the
// pre-dedup behavior). Used by promoteExtraction to skip near-identical
// intentions.
func (s *MemoryStore) TopL7Similar(goal string, userID, agentID string, topN int) (float32, bool) {
	col := s.cols[memory.L7Intention]
	n := col.Count()
	if n <= 0 {
		return 0, false
	}
	if topN <= 0 || topN > n {
		topN = n
	}
	where := map[string]string{}
	if userID != "" {
		where["user_id"] = userID
	}
	if agentID != "" {
		where["agent_id"] = agentID
	}
	if len(where) == 0 {
		where = nil
	}
	// chromem requires k <= total doc count; request only the top-N docs
	// (cheaper) and take the best within the scope-filtered results.
	res, err := col.Query(s.ctx, goal, topN, where, nil)
	if err != nil || len(res) == 0 {
		return 0, false
	}
	return res[0].Similarity, true
}

// EnsureL7Max enforces a hard cap on the L7 intention layer: when the active
// doc count (scoped to user/agent, or global when both are empty) exceeds
// max, the OLDEST docs (by ts metadata, ties broken by id) are deleted until
// the count fits. The incoming goal is not yet written at call time, so the
// caller counts against (max-1) if it wants a hard guarantee including the
// new write; we use max as the ceiling. Returns the number of docs evicted.
// LRU semantics keep the most recent intentions (they encode the user's
// current standing goals; stale ones from weeks ago are the bloat source).
func (s *MemoryStore) EnsureL7Max(goal, userID, agentID string, max int) int {
	s.mu.Lock()
	defer s.mu.Unlock()
	var inScope []DocIndex
	for _, d := range s.index {
		if d.Layer != string(memory.L7Intention) {
			continue
		}
		if userID != "" && d.UserID != userID {
			continue
		}
		if agentID != "" && d.AgentID != agentID {
			continue
		}
		inScope = append(inScope, d)
	}
	if len(inScope) <= max {
		return 0
	}
	sort.Slice(inScope, func(i, j int) bool {
		// oldest first (ts asc, id asc as tiebreak)
		if inScope[i].Ts != inScope[j].Ts {
			return inScope[i].Ts < inScope[j].Ts
		}
		return inScope[i].ID < inScope[j].ID
	})
	over := len(inScope) - max
	deleted := 0
	for i := 0; i < over; i++ {
		d := inScope[i]
		if col, ok := s.cols[memory.L7Intention]; ok {
			_ = col.Delete(s.ctx, nil, nil, d.ID)
		}
		delete(s.index, d.ID)
		deleted++
		log.Printf("EnsureL7Max: evicted old L7 intention %s ts=%s (%d/%d over cap %d)",
			d.ID, d.Ts, i+1, over, max)
	}
	return deleted
}

// List returns exact-match docs, optionally filtered by layer/user/agent, with pagination.
func (s *MemoryStore) List(layer memory.Layer, userID, agentID string, limit, offset int) ([]DocIndex, int) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	var all []DocIndex
	for _, d := range s.index {
		if layer != "" && d.Layer != string(layer) {
			continue
		}
		if userID != "" && d.UserID != userID {
			continue
		}
		if agentID != "" && d.AgentID != agentID {
			continue
		}
		all = append(all, d)
	}
	// stable sort by ts desc
	sort.Slice(all, func(i, j int) bool { return all[i].Ts > all[j].Ts })
	total := len(all)
	if offset > total {
		offset = total
	}
	end := offset + limit
	if end > total {
		end = total
	}
	if offset > end {
		offset = end
	}
	return all[offset:end], total
}

// Delete removes docs by id (or by layer/user/agent scope). Returns count deleted.
func (s *MemoryStore) Delete(ids []string, layer memory.Layer, userID, agentID string) (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	targets := map[string]bool{}
	if len(ids) > 0 {
		for _, id := range ids {
			if _, ok := s.index[id]; ok {
				targets[id] = true
			}
		}
	} else {
		for id, d := range s.index {
			if layer != "" && d.Layer != string(layer) {
				continue
			}
			if userID != "" && d.UserID != userID {
				continue
			}
			if agentID != "" && d.AgentID != agentID {
				continue
			}
			targets[id] = true
		}
	}
	deleted := 0
	for id := range targets {
		d := s.index[id]
		if col, ok := s.cols[memory.Layer(d.Layer)]; ok {
			_ = col.Delete(s.ctx, nil, nil, id)
		}
		delete(s.index, id)
		deleted++
	}
	return deleted, s.persistIndexLocked()
}

// GetDoc returns one document from the exact index.
func (s *MemoryStore) GetDoc(id string) (DocIndex, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	d, ok := s.index[id]
	return d, ok
}

// LayerCounts returns the number of docs per layer (exact).
func (s *MemoryStore) LayerCounts() map[string]int {
	s.mu.RLock()
	defer s.mu.RUnlock()
	out := map[string]int{}
	for _, l := range memory.All() {
		out[string(l)] = 0
	}
	for _, d := range s.index {
		out[d.Layer]++
	}
	return out
}

// TotalMemories sums all layer docs.
func (s *MemoryStore) TotalMemories() int {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return len(s.index)
}

// SetExtracted marks a doc as extracted (used after successful promotion).
func (s *MemoryStore) SetExtracted(id string, v bool) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if d, ok := s.index[id]; ok {
		d.Extracted = v
		s.index[id] = d
	}
	return s.persistIndexLocked()
}

// Graph exposes the L5 knowledge graph.
func (s *MemoryStore) Graph() *graph.Store { return s.g }

// ---- usage counters ----

// Usage returns the current atomic counters (writes, searches) — read-only
// snapshot for /api/v1/status. Used by the desktop pane to show whether the
// memory system is actually being read.
func (s *MemoryStore) Usage() (writes, searches uint64) {
	return s.writes.Load(), s.searches.Load()
}

// UsageForJSON flattens Usage into a single map for embedding in
// /api/v1/status, /api/info, and /api/layer-counts. JSON must serialize
// uint64 as a number, and (w, s) tuples do not.
func (s *MemoryStore) UsageForJSON() map[string]uint64 {
	w, q := s.Usage()
	return map[string]uint64{"writes": w, "searches": q}
}

func (s *MemoryStore) loadUsage() {
	if s.countsPath == "" {
		return
	}
	b, err := os.ReadFile(s.countsPath)
	if err != nil || len(b) == 0 {
		return
	}
	var c UsageCounters
	if err := json.Unmarshal(b, &c); err != nil {
		return
	}
	s.writes.Store(c.Writes)
	s.searches.Store(c.Searches)
}

// persistUsageAsync writes the counters without blocking the request path.
// One pending write at a time; the latest call's snapshot wins.
func (s *MemoryStore) persistUsageAsync() {
	if s.countsPath == "" {
		return
	}
	go func() {
		s.persistUsage()
	}()
}

// Close drains any pending async persistence before the store is discarded.
// Safe to call multiple times.
func (s *MemoryStore) Close() {
	// Stop accepting deferred flushes so persistIndex falls back to sync writes.
	s.indexFlushMu.Lock()
	if !s.indexFlushed {
		s.indexFlushed = true
		close(s.indexFlushCh)
	}
	if s.indexTimer != nil {
		s.indexTimer.Stop()
		s.indexTimer = nil
	}
	dirty := s.indexDirty
	s.indexDirty = false
	s.indexFlushMu.Unlock()
	// If there was an unflushed dirty batch, drain it now before we tear down.
	if dirty {
		s.mu.RLock()
		if err := s.persistIndexLocked(); err != nil {
			log.Printf("hyatlas: final index flush failed: %v", err)
		}
		s.mu.RUnlock()
	}
	s.persistUsage() // sync; wait for any in-flight goroutine
}

func (s *MemoryStore) persistUsage() error {
	if s.countsPath == "" {
		return nil
	}
	c := UsageCounters{Writes: s.writes.Load(), Searches: s.searches.Load()}
	data, err := json.MarshalIndent(c, "", "  ")
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(s.countsPath), 0o755); err != nil {
		return err
	}
	tmp := s.countsPath + ".tmp"
	if err := os.WriteFile(tmp, data, 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, s.countsPath)
}

// ---- index persistence ----

func docIndexFrom(id, layer, content string, meta map[string]string) DocIndex {
	d := DocIndex{ID: id, Layer: layer, Content: content}
	if meta != nil {
		d.UserID = meta["user_id"]
		d.AgentID = meta["agent_id"]
		d.Ts = meta["ts"]
		d.Extracted = meta["extracted"] == "true"
		d.Meta = meta
	}
	return d
}

func (s *MemoryStore) persistIndex() error {
	// Defer to scheduleIndexFlush if the flush channel is still open (i.e. the
	// store isn't shutting down). Otherwise fall through to a direct write.
	select {
	case <-s.indexFlushCh:
		s.mu.RLock()
		defer s.mu.RUnlock()
		return s.persistIndexLocked()
	default:
	}
	return s.scheduleIndexFlush()
}

// scheduleIndexFlush marks the index dirty and resets a debounced timer. Every
// call within the flush interval collapses into a single disk write — this is
// the whole point of the coalescing.
func (s *MemoryStore) scheduleIndexFlush() error {
	s.indexFlushMu.Lock()
	s.indexDirty = true
	if s.indexTimer != nil {
		s.indexTimer.Stop()
	}
	s.indexTimer = time.AfterFunc(s.indexFlushInterval(), func() {
		s.flushIndex()
	})
	s.indexFlushMu.Unlock()
	return nil
}

// indexFlushInterval returns the coalescing window from HYATLAS_INDEX_FLUSH_SEC.
// Defaults to 1s; set 0 (or negative) to disable coalescing (legacy sync mode).
func (s *MemoryStore) indexFlushInterval() time.Duration {
	if v := os.Getenv("HYATLAS_INDEX_FLUSH_SEC"); v != "" {
		if n, err := strconv.Atoi(v); err == nil {
			return time.Duration(n) * time.Second
		}
	}
	return time.Second
}

// flushIndex performs the actual disk write if dirty, then clears the dirty flag.
func (s *MemoryStore) flushIndex() {
	s.indexFlushMu.Lock()
	dirty := s.indexDirty
	s.indexDirty = false
	s.indexTimer = nil
	s.indexFlushMu.Unlock()
	if !dirty {
		return
	}
	// Hold the read lock during the actual write so the map we serialize is a
	// consistent snapshot. Writes take the write lock and go through the
	// schedule path, so no race here.
	s.mu.RLock()
	defer s.mu.RUnlock()
	if err := s.persistIndexLocked(); err != nil {
		log.Printf("hyatlas: index flush failed: %v", err)
	}
}

// persistIndexLocked writes the index assuming the caller already holds s.mu.
// It does NOT take any lock (avoids double-lock / RLock-while-WriteLock deadlocks).
func (s *MemoryStore) persistIndexLocked() error {
	if s.indexPath == "" {
		return nil
	}
	data, err := json.MarshalIndent(s.index, "", "  ")
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(s.indexPath), 0o755); err != nil {
		return err
	}
	tmp := s.indexPath + ".tmp"
	if err := os.WriteFile(tmp, data, 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, s.indexPath)
}

// rebuildIndex reconstructs the exact index from chromem by enumerating all docs.
func (s *MemoryStore) rebuildIndex() error {
	// chromem has no "list all"; enumerate via QueryEmbedding with zero vector over each layer.
	for _, l := range memory.All() {
		col := s.cols[l]
		n := col.Count()
		if n == 0 {
			continue
		}
		// zero vector queries return all docs (score ~0) in arbitrary order.
		res, err := col.QueryEmbedding(s.ctx, make([]float32, s.dimFor(l)), n, nil, nil)
		if err != nil {
			// if zero-vector fails, fall back to a neutral query
			res, err = col.Query(s.ctx, "memory", n, nil, nil)
			if err != nil {
				continue
			}
		}
		for _, r := range res {
			s.index[r.ID] = docIndexFrom(r.ID, string(l), r.Content, r.Metadata)
		}
	}
	return s.persistIndex()
}

func (s *MemoryStore) dimFor(l memory.Layer) int {
	if s.dims > 0 {
		return s.dims
	}
	return 384
}
