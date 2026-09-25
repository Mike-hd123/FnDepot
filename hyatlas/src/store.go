package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
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
	// closedFlag flips in Close(). After that, persistIndex/persistUsageAsync/
	// flushIndex refuse to touch the disk: the final drain in Close() already
	// wrote everything. Without this guard the debounced timer AND the
	// persistUsageAsync goroutine (whose "wait for in-flight" comment was
	// never backed by a lock) can recreate a just-removed temp dir — the
	// proven flake behind `t.TempDir cleanup: directory not empty`
	// (see metadata tests, 2026-09-17).
	closedFlag   atomic.Bool
	usageWriteMu sync.Mutex // serialises counters writes vs Close's final drain
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

// Search does vector search, scoped to user/agent when provided. Expired
// docs (valid_until in the past, see validUntilExpired) are filtered out —
// pass true to include them (e.g. for the supersede "neighbors" lookup and
// audit/dream tooling).
func (s *MemoryStore) Search(query string, limit int, layer memory.Layer, userID, agentID string, includeExpired bool) ([]SearchHit, error) {
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
			if !includeExpired && validUntilExpired(r.Metadata["valid_until"]) {
				continue
			}
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

// ---------------------------------------------------------------------------
// Metadata layer (memory-optimization plan v2 §3 ①②③)
//
// Truth source is the chromem document metadata: rebuildIndex() overwrites
// the derived doc index from chromem on every startup, so any field that
// must survive a restart has to live in chromem. Every mutation below
// therefore double-writes: chromem AddDocument (same-ID upsert) + s.index.
//
// chromem@v0.7.0 semantics relied on here (verified against module source):
//   - GetByID returns a CLONE including the stored Embedding
//   - AddDocument with an existing ID replaces the map entry in place and,
//     for persistent collections, rewrites only that document's file
//   - AddDocument skips (re-)embedding entirely when Embedding is non-empty
//     => metadata updates never touch the embedder (no onnx / network cost).
// ---------------------------------------------------------------------------

// TouchIDs records a retrieval hit for the given ids: last_hit_at=now (RFC3339
// UTC) and hit_count+1. Returns the number of docs updated. Unknown ids are
// skipped silently (they may have been deleted concurrently).
func (s *MemoryStore) TouchIDs(ids []string) (int, error) {
	now := time.Now().UTC().Format(time.RFC3339)
	touched := 0
	for _, id := range ids {
		d, ok := s.GetDoc(id)
		if !ok {
			continue
		}
		col, ok := s.cols[memory.Layer(d.Layer)]
		if !ok {
			continue
		}
		doc, err := col.GetByID(s.ctx, id)
		if err != nil {
			continue
		}
		if doc.Metadata == nil {
			doc.Metadata = map[string]string{}
		}
		doc.Metadata["last_hit_at"] = now
		doc.Metadata["hit_count"] = strconv.FormatInt(parseMetaInt(doc.Metadata["hit_count"])+1, 10)
		if err := col.AddDocument(s.ctx, doc); err != nil {
			return touched, err
		}
		s.applyMetaToIndex(id, map[string]string{
			"last_hit_at": doc.Metadata["last_hit_at"],
			"hit_count":   doc.Metadata["hit_count"],
		}, nil)
		touched++
	}
	if touched > 0 {
		if err := s.persistIndex(); err != nil {
			return touched, err
		}
	}
	return touched, nil
}

// PatchMeta applies a neutral key/value metadata patch to one doc: set wins
// over clear for the same key. layer/ts/user_id/agent_id are immutable.
// Returns ErrNotFound when the id is not indexed, and (found, oldValues) for
// every requested key so callers can roll back.
func (s *MemoryStore) PatchMeta(id string, set map[string]string, clear []string) (bool, map[string]string, error) {
	d, ok := s.GetDoc(id)
	if !ok {
		return false, nil, errNotFound
	}
	col, ok := s.cols[memory.Layer(d.Layer)]
	if !ok {
		return false, nil, fmt.Errorf("no collection for layer %q", d.Layer)
	}
	doc, err := col.GetByID(s.ctx, id)
	if err != nil {
		return false, nil, fmt.Errorf("doc %s not found in store: %w", id, err)
	}
	if doc.Metadata == nil {
		doc.Metadata = map[string]string{}
	}
	old := map[string]string{}
	for k, v := range set {
		if metaImmutable[k] {
			continue
		}
		old[k] = doc.Metadata[k]
		doc.Metadata[k] = v
	}
	for _, k := range clear {
		if metaImmutable[k] {
			continue
		}
		if _, done := old[k]; !done {
			old[k] = doc.Metadata[k]
		}
		delete(doc.Metadata, k)
	}
	if err := col.AddDocument(s.ctx, doc); err != nil {
		return false, nil, err
	}
	s.applyMetaToIndex(id, set, clear)
	return true, old, s.persistIndex()
}

var errNotFound = errors.New("not found")

// metaImmutable keys are owned by the write path and the index projection;
// /patch must never rewrite them (the derived DocIndex fields would drift).
var metaImmutable = map[string]bool{"layer": true, "ts": true, "user_id": true, "agent_id": true}

// applyMetaToIndex mirrors a metadata change onto the derived doc index. Takes
// the write lock; callers must not hold s.mu.
func (s *MemoryStore) applyMetaToIndex(id string, set map[string]string, clear []string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	d, ok := s.index[id]
	if !ok {
		return
	}
	meta := make(map[string]string, len(d.Meta)+len(set))
	for k, v := range d.Meta {
		meta[k] = v
	}
	for _, k := range clear {
		if !metaImmutable[k] {
			delete(meta, k)
		}
	}
	for k, v := range set {
		if !metaImmutable[k] {
			meta[k] = v
		}
	}
	d.Meta = meta
	s.index[id] = d
}

// validUntilExpired reports whether an RFC3339 valid_until is in the past.
// A "!force" suffix (set by /patch ?force=1 when lowering valid_until below
// gmt_created) always returns false — forced values must never hide content.
// Empty string or unparsable value also return false — fail-open, mirroring
// the L7 dedup error style.
func validUntilExpired(s string) bool {
	if strings.HasSuffix(s, "!force") {
		return false
	}
	if s == "" {
		return false
	}
	t, err := time.Parse(time.RFC3339, s)
	if err != nil {
		return false
	}
	return t.Before(time.Now())
}

func parseMetaInt(s string) int64 {
	n, err := strconv.ParseInt(s, 10, 64)
	if err != nil || n < 0 {
		return 0
	}
	return n
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
// One pending write at a time; the latest call's snapshot wins. After Close()
// it is a no-op (guarded by closedFlag under usageWriteMu, see persistUsage).
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
	// Flip the stop flag under usageWriteMu so any in-flight persistUsageAsync
	// goroutine either finished before us or sees closedFlag and no-ops. The
	// old code only called persistUsage() once at the end and never actually
	// waited — a late goroutine could recreate a t.TempDir already cleaned up.
	s.usageWriteMu.Lock()
	s.closedFlag.Store(true)
	s.usageWriteMu.Unlock()
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
	// Write-lock (not RLock): this also waits for any in-flight flushIndex()
	// that passed its first closedFlag check but is still holding RLock, so
	// the final on-disk state is a superset of anything it could have written.
	if dirty {
		s.mu.Lock()
		if err := s.persistIndexLocked(); err != nil {
			log.Printf("hyatlas: final index flush failed: %v", err)
		}
		s.mu.Unlock()
	}
	s.persistUsageInner(true) // force final counters write despite closedFlag
}

func (s *MemoryStore) persistUsage() error {
	return s.persistUsageInner(false)
}

// persistUsageInner writes the counters while holding usageWriteMu. Unless
// force is set (Close's final drain), it no-ops after Close so a late
// async caller cannot recreate an already-removed data dir.
func (s *MemoryStore) persistUsageInner(force bool) error {
	s.usageWriteMu.Lock()
	defer s.usageWriteMu.Unlock()
	if !force && s.closedFlag.Load() {
		return nil
	}
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
	if meta == nil {
		// Preserve the old contract: a nil bag stays a nil Meta (consumers
		// branch on it.Meta != nil). Add() writes the true chromem doc with a
		// non-nil bag anyway, so this only covers direct index construction.
		return d
	}
	d.UserID = meta["user_id"]
	d.AgentID = meta["agent_id"]
	d.Ts = meta["ts"]
	d.Extracted = meta["extracted"] == "true"
	// P1.5 L2 layer-label self-heal (impl-spec §5.2#1): the collection prefix
	// is the ground truth for a doc's layer; doc_index.json consumers (the
	// Python promote filter meta.layer=="l2_raw", /list, /patch reads) had no
	// guarantee the label existed — docs written straight into chromem (v3
	// legacy / shadow pipeline) came back from rebuildIndex with no meta.layer
	// at all (shadow 09-05: 256/269 l2_raw docs missing the label), and a bad
	// or missing label silently misroutes them through the layer gates. Every
	// construction path — Add and rebuildIndex on each startup — funnels here,
	// so persisting a corrected label heals old files on the next restart
	// without a migration.
	if meta["layer"] != layer {
		meta["layer"] = layer
	}
	d.Meta = meta
	return d
}

func (s *MemoryStore) persistIndex() error {
	// After Close(), doc_index.json writes are dropped: chromem is the source
	// of truth and rebuildIndex restores the exact index on next startup, so
	// a late async caller must not recreate an already-removed data dir.
	if s.closedFlag.Load() {
		return nil
	}
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
	if !dirty || s.closedFlag.Load() {
		return
	}
	// Hold the read lock during the actual write so the map we serialize is a
	// consistent snapshot. Writes take the write lock and go through the
	// schedule path, so no race here.
	s.mu.RLock()
	defer s.mu.RUnlock()
	// Re-check under the lock: Close()'s s.mu.Lock() drain guarantees that
	// once we hold RLock here and the flag is still clear, Close hasn't
	// started its final flush — otherwise it wins and we must not write.
	if s.closedFlag.Load() {
		return
	}
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
//
// Stability hardening (2026-09-26, t_a7ff4ed2):
//  1. every per-layer failure path logs WARNING/ERROR — the old silent
//     `continue` is how a 384d-vs-1024d dim mismatch wiped the index with zero
//     diagnostics (chromem returns "vectors must have the same length").
//  2. overwrite guard: if this rebuild indexed < 50% of what chromem actually
//     holds (or < 50% of the previous on-disk index), persist is REFUSED and
//     the previous doc_index.json stays untouched. chromem remains the source
//     of truth; a broken rebuild must not clobber a good index.
//  3. the text-query fallback embeds via the configured embedder, which may be
//     a remote wrapper still starting up — wait for it (up to 60s, exponential
//     backoff) before giving up a layer.
func (s *MemoryStore) rebuildIndex() error {
	// chromem has no "list all"; enumerate via QueryEmbedding with zero vector over each layer.
	expected := 0
	started := len(s.index) // normally 0; entries already in the map count as indexed
	var embedWaited, embedReady bool
	var failedLayers []string
	for _, l := range memory.All() {
		col := s.cols[l]
		n := col.Count()
		if n == 0 {
			continue
		}
		expected += n
		// zero vector queries return all docs (score ~0) in arbitrary order.
		res, err := col.QueryEmbedding(s.ctx, make([]float32, s.dimFor(l)), n, nil, nil)
		if err != nil {
			log.Printf("WARNING hyatlas: rebuildIndex %s: zero-vector query failed for %d docs (query dim=%d): %v",
				l, n, s.dimFor(l), err)
			// if zero-vector fails, fall back to a neutral query. The fallback
			// embeds through the configured embedder — give a slow-to-start
			// remote embedder a chance before failing the layer (fix 3).
			if s.needsEmbedderProbe() && !embedWaited {
				embedWaited = true
				embedReady = s.waitForEmbedder(60 * time.Second)
			}
			res, err = col.Query(s.ctx, "memory", n, nil, nil)
			if err != nil {
				log.Printf("ERROR hyatlas: rebuildIndex %s: fallback query failed, %d docs NOT indexed (embedder ready=%v): %v",
					l, n, embedReady, err)
				failedLayers = append(failedLayers, fmt.Sprintf("%s(%d docs)", l, n))
				continue
			}
		}
		for _, r := range res {
			s.index[r.ID] = docIndexFrom(r.ID, string(l), r.Content, r.Metadata)
		}
	}
	indexed := len(s.index) - started

	// --- fix 2: overwrite guard ---
	// Both references are "how much SHOULD be in the index": the live chromem
	// doc count (expected) and the previous persisted index (oldCount). If the
	// rebuild recovered less than half of either, refuse to persist.
	if reason := s.persistGuard(indexed, expected); reason != "" {
		log.Printf("ERROR hyatlas: rebuildIndex REFUSED to overwrite doc_index.json: %s (indexed=%d expected=%d failedLayers=%v) — previous index kept; fix the embedder/dims config and restart",
			reason, indexed, expected, failedLayers)
		return nil
	}
	if len(failedLayers) > 0 {
		log.Printf("WARNING hyatlas: rebuildIndex completed with %d failed layer(s) %v (%d/%d docs indexed)",
			len(failedLayers), failedLayers, indexed, expected)
	}
	return s.persistIndex()
}

// persistGuard returns a non-empty refusal reason when writing an index of
// `indexed` entries would destroy more than half of either reference count:
// the live chromem doc total (expected) or the previous on-disk index.
// Empty string = safe to persist. A missing/unparseable old index is not a
// reason (there is nothing worth protecting).
func (s *MemoryStore) persistGuard(indexed, expected int) string {
	if expected > 0 && indexed*2 < expected {
		return "recovered <50% of chromem docs"
	}
	if oldCount := countIndexFile(s.indexPath); oldCount > 0 && indexed*2 < oldCount {
		return fmt.Sprintf("new index <50%% of previous doc_index.json (%d entries)", oldCount)
	}
	return ""
}

// countIndexFile cheaply counts top-level keys of a persisted doc index
// (values decoded as RawMessage so the 94 MB file parse stays fast).
// Returns 0 when the file is missing or unreadable.
func countIndexFile(path string) int {
	b, err := os.ReadFile(path)
	if err != nil {
		return 0
	}
	var m map[string]json.RawMessage
	if err := json.Unmarshal(b, &m); err != nil {
		return 0
	}
	return len(m)
}

// needsEmbedderProbe reports whether the configured embedder talks to an
// external process (whose startup can race with ours). Embedded/local models
// are in-process and always ready by the time rebuildIndex runs.
func (s *MemoryStore) needsEmbedderProbe() bool {
	_, remote := s.embed.(*OpenAIEmbedder)
	return remote
}

// waitForEmbedder probes the embedder with exponential backoff (500ms..5s)
// until it answers or maxWait elapses. Returns true when the probe succeeded.
func (s *MemoryStore) waitForEmbedder(maxWait time.Duration) bool {
	deadline := time.Now().Add(maxWait)
	backoff := 500 * time.Millisecond
	for {
		_, err := s.embed.Embed(s.ctx, "hyatlas startup probe")
		if err == nil {
			return true
		}
		remaining := time.Until(deadline)
		if remaining <= 0 {
			log.Printf("WARNING hyatlas: embedder not ready after %v, giving up waiting: %v", maxWait, err)
			return false
		}
		if backoff > remaining {
			backoff = remaining
		}
		log.Printf("INFO hyatlas: waiting for embedder (%v): %v", backoff, err)
		time.Sleep(backoff)
		backoff *= 2
		if backoff > 5*time.Second {
			backoff = 5 * time.Second
		}
	}
}

func (s *MemoryStore) dimFor(l memory.Layer) int {
	if s.dims > 0 {
		return s.dims
	}
	return 384
}
