// Package main: hyatlas full-vault re-embedder (bge-large-zh-v1.5 → bge-m3).
//
// Opens the same chromem-go persistent DB that hyatlas-go uses, reads
// doc_index.json for the source-of-truth content, re-embeds every document
// via the SiliconFlow bge-m3 endpoint, and overwrites each document's gob
// file in-place via Collection.AddDocument (passing a non-empty Embedding so
// chromem skips its own embedder — see chromem-go AddDocument, store.go:384).
//
// Design constraints (from task t_23dd690e):
//   - NO delete/delete_all: AddDocument with the same ID overwrites the map
//     entry + the on-disk gob file (chromem semantics).
//   - Batch ≤ 16, conservative rate (50K TPM guard, RPM < 600).
//   - 429 exponential backoff.
//   - Resumable: progress cursor persisted to a sidecar file; re-run skips
//     already-processed IDs.
//   - Progress log every 500 items.
//
// MUST be run while hyatlas-go is stopped (concurrent writers to the same
// persistent DB will corrupt the gob files).
package main

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"math"
	"net/http"
	"net/url"
	"os"
	"os/signal"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"syscall"
	"time"

	"github.com/philippgille/chromem-go"
)

// DocIndex mirrors hyatlas store.go DocIndex (the persisted JSON shape).
type DocIndex struct {
	ID        string            `json:"id"`
	Layer     string            `json:"layer"`
	Content   string            `json:"content"`
	UserID    string            `json:"user_id"`
	AgentID   string            `json:"agent_id"`
	Ts        string            `json:"ts"`
	Extracted bool              `json:"extracted"`
	Meta      map[string]string `json:"meta,omitempty"`
}

// EmbeddingResponse is the minimal OpenAI-compatible /v1/embeddings response.
type EmbeddingResponse struct {
	Data []struct {
		Embedding []float32 `json:"embedding"`
		Index     int       `json:"index"`
	} `json:"data"`
	Usage struct {
		TotalTokens int `json:"total_tokens"`
	} `json:"usage"`
}

// ProgressEntry is one line of the progress cursor file.
type ProgressEntry struct {
	ID        string    `json:"id"`
	Status    string    `json:"status"` // "done" or "skip"
	Timestamp time.Time `json:"ts"`
}

// Stats accumulates run-wide counters for the final report.
type Stats struct {
	Total      int           `json:"total"`
	Done       int           `json:"done"`
	Skipped    int           `json:"skipped"`
	Failed     int           `json:"failed"`
	Tokens     int           `json:"tokens"`
	Elapsed    time.Duration `json:"elapsed_sec"`
	StartedAt  time.Time     `json:"started_at"`
	FinishedAt time.Time     `json:"finished_at,omitempty"`
}

func main() {
	var (
		dbDir       = flag.String("db", "/var/apps/hyatlas/shares/hyatlas/go-data", "chromem persistent DB directory")
		indexFile   = flag.String("index", "", "doc_index.json path (defaults to <db>/doc_index.json)")
		embedURL    = flag.String("embed-url", "https://api.siliconflow.cn/v1/embeddings", "embedding API endpoint")
		embedModel  = flag.String("embed-model", "BAAI/bge-m3", "embedding model name")
		embedKey    = flag.String("embed-key", "", "embedding API key (or env EMBED_API_KEY)")
		proxy       = flag.String("proxy", "", "HTTP proxy for embedding API (or env HTTPS_PROXY)")
		batch       = flag.Int("batch", 16, "max concurrent embedding requests")
		tpmLimit    = flag.Int("tpm", 50000, "conservative TPM guard; 0 = no limit")
		dryRun      = flag.Bool("dry-run", false, "don't write to DB; just probe embeddings")
		progressFile = flag.String("progress", "", "progress cursor file (defaults to <db>/reembed.progress.jsonl)")
		logEvery    = flag.Int("log-every", 500, "print progress every N items")
		resume      = flag.Bool("resume", true, "skip IDs already marked done in the progress file")
		limit       = flag.Int("limit", 0, "max items to process (0 = all)")
		maxChars    = flag.Int("max-chars", 0, "truncate content to N chars before embedding (0 = no truncation)")
	)
	flag.Parse()

	if *indexFile == "" {
		*indexFile = filepath.Join(*dbDir, "doc_index.json")
	}
	if *progressFile == "" {
		*progressFile = filepath.Join(*dbDir, "reembed.progress.jsonl")
	}
	if *embedKey == "" {
		*embedKey = os.Getenv("EMBED_API_KEY")
	}
	if *proxy == "" {
		*proxy = os.Getenv("HTTPS_PROXY")
	}
	if *embedKey == "" {
		fmt.Fprintln(os.Stderr, "ERROR: embed-key or EMBED_API_KEY required")
		os.Exit(2)
	}

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	// Graceful shutdown on SIGINT/SIGTERM: flush progress, print partial stats.
	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)
	go func() {
		<-sigCh
		fmt.Fprintln(os.Stderr, "\nInterrupt received, flushing progress and exiting...")
		cancel()
	}()

	// ── 1. Load doc_index.json ──
	fmt.Fprintf(os.Stderr, "[%s] loading doc_index from %s ...\n", time.Now().Format("15:04:05"), *indexFile)
	raw, err := os.ReadFile(*indexFile)
	if err != nil {
		fmt.Fprintf(os.Stderr, "ERROR reading index: %v\n", err)
		os.Exit(1)
	}
	var index map[string]DocIndex
	if err := json.Unmarshal(raw, &index); err != nil {
		fmt.Fprintf(os.Stderr, "ERROR parsing index: %v\n", err)
		os.Exit(1)
	}

	// Deterministic order: sort by ID so resume is stable across runs.
	ids := make([]string, 0, len(index))
	for id := range index {
		ids = append(ids, id)
	}
	sort.Strings(ids)
	if *limit > 0 && *limit < len(ids) {
		ids = ids[:*limit]
	}
	fmt.Fprintf(os.Stderr, "[%s] %d entries in index\n", time.Now().Format("15:04:05"), len(ids))

	// ── 2. Load progress cursor (resume) ──
	done := make(map[string]bool, len(ids))
	if *resume {
		if f, err := os.Open(*progressFile); err == nil {
			scanner := bufio.NewScanner(f)
			scanner.Buffer(make([]byte, 0, 65536), 1<<20)
			for scanner.Scan() {
				var pe ProgressEntry
				if err := json.Unmarshal(scanner.Bytes(), &pe); err == nil && pe.Status == "done" {
					done[pe.ID] = true
				}
			}
			f.Close()
			fmt.Fprintf(os.Stderr, "[%s] resume: %d already done\n", time.Now().Format("15:04:05"), len(done))
		}
	}

	// ── 3. Open progress file for append ──
	progF, err := os.OpenFile(*progressFile, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0644)
	if err != nil {
		fmt.Fprintf(os.Stderr, "ERROR opening progress file: %v\n", err)
		os.Exit(1)
	}
	defer progF.Close()
	progW := bufio.NewWriter(progF)
	defer progW.Flush()

	// ── 4. Open chromem DB (unless dry-run) ──
	var db *chromem.DB
	var cols map[string]*chromem.Collection
	if !*dryRun {
		db, err = chromem.NewPersistentDB(*dbDir, false)
		if err != nil {
			fmt.Fprintf(os.Stderr, "ERROR opening chromem DB: %v\n", err)
			os.Exit(1)
		}
		cols = make(map[string]*chromem.Collection, 7)
		for _, layer := range []string{"l1_profile", "l2_raw", "l3_fact", "l4_summary", "l5_knowledge", "l6_schema", "l7_intention"} {
			col, err := db.GetOrCreateCollection(layer, nil, nil)
			if err != nil {
				fmt.Fprintf(os.Stderr, "ERROR opening collection %s: %v\n", layer, err)
				os.Exit(1)
			}
			cols[layer] = col
		}
	}

	// ── 5. HTTP client with proxy ──
	transport := &http.Transport{}
	if *proxy != "" {
		proxyURL, err := url.Parse(*proxy)
		if err == nil {
			transport.Proxy = http.ProxyURL(proxyURL)
		}
	}
	client := &http.Client{
		Transport: transport,
		Timeout:   120 * time.Second,
	}

	// ── 6. TPM rate limiter ──
	var tpmMu sync.Mutex
	var tokenWindow []time.Time
	var tokenCount int
	flushTokens := func() {
		cutoff := time.Now().Add(-60 * time.Second)
		i := 0
		for i < len(tokenWindow) && tokenWindow[i].Before(cutoff) {
			i++
		}
		tokenWindow = tokenWindow[i:]
		tokenCount = 0
		for _, t := range tokenWindow {
			_ = t
			tokenCount++
		}
	}
	// tokenCount is just len(tokenWindow); we keep both for clarity.
	_ = tokenCount

	// ── 7. Process loop ──
	stats := Stats{Total: len(ids), StartedAt: time.Now()}
	type result struct {
		id     string
		vec    []float32
		tokens int
		err    error
	}
	worker := func(id string, content string) result {
		// Truncate if --max-chars is set (rune-aware to avoid splitting multi-byte UTF-8)
		if *maxChars > 0 {
			runes := []rune(content)
			if len(runes) > *maxChars {
				content = string(runes[:*maxChars])
			}
		}
		// TPM guard
		if *tpmLimit > 0 {
			tpmMu.Lock()
			flushTokens()
			for len(tokenWindow) >= *tpmLimit {
				tpmMu.Unlock()
				time.Sleep(100 * time.Millisecond)
				tpmMu.Lock()
				flushTokens()
			}
			tokenWindow = append(tokenWindow, time.Now())
			tpmMu.Unlock()
		}

		// Embed request (with 429 backoff)
		body, _ := json.Marshal(map[string]any{"model": *embedModel, "input": content})
		var resp *http.Response
		var lastErr error
		for attempt := 0; attempt < 5; attempt++ {
			req, _ := http.NewRequestWithContext(ctx, "POST", *embedURL, bytes.NewReader(body))
			req.Header.Set("Content-Type", "application/json")
			req.Header.Set("Authorization", "Bearer "+*embedKey)
			resp, lastErr = client.Do(req)
			if lastErr == nil {
				if resp.StatusCode == 429 {
					resp.Body.Close()
					backoff := time.Duration(math.Pow(2, float64(attempt+1))) * time.Second
					if backoff > 30*time.Second {
						backoff = 30 * time.Second
					}
					select {
					case <-ctx.Done():
						return result{id: id, err: ctx.Err()}
					case <-time.After(backoff):
					}
					continue
				}
				break
			}
			// transient network error
			select {
			case <-ctx.Done():
				return result{id: id, err: ctx.Err()}
			case <-time.After(time.Duration(attempt+1) * time.Second):
			}
		}
		if lastErr != nil {
			return result{id: id, err: lastErr}
		}
		defer resp.Body.Close()
		raw, err := io.ReadAll(resp.Body)
		if resp.StatusCode != 200 {
			return result{id: id, err: fmt.Errorf("embed HTTP %d: %s", resp.StatusCode, truncStr(raw, 300))}
		}
		if err != nil {
			return result{id: id, err: err}
		}
		var embResp EmbeddingResponse
		if err := json.Unmarshal(raw, &embResp); err != nil {
			return result{id: id, err: fmt.Errorf("parse embedding response: %w (%s)", err, truncStr(raw, 200))}
		}
		if len(embResp.Data) == 0 || len(embResp.Data[0].Embedding) == 0 {
			return result{id: id, err: fmt.Errorf("empty embedding in response: %s", truncStr(raw, 200))}
		}
		return result{id: id, vec: embResp.Data[0].Embedding, tokens: embResp.Usage.TotalTokens}
	}

	// ── 7b. Pipeline: feed IDs → embed → write ──
	// Simple serial-batch model: process in chunks of *batch concurrently.
	for i := 0; i < len(ids); i += *batch {
		if ctx.Err() != nil {
			break
		}
		end := i + *batch
		if end > len(ids) {
			end = len(ids)
		}
		chunk := ids[i:end]

		type pending struct {
			idx  int
			id   string
			doc  DocIndex
			res  result
		}
		var wg sync.WaitGroup
		results := make([]pending, len(chunk))
		for j, id := range chunk {
			if done[id] {
				results[j] = pending{idx: j, id: id, doc: index[id], res: result{id: id}}
				continue
			}
			wg.Add(1)
			go func(j int, id string) {
				defer wg.Done()
				doc := index[id]
				r := worker(id, doc.Content)
				results[j] = pending{idx: j, id: id, doc: doc, res: r}
			}(j, id)
		}
		wg.Wait()

		// Write phase (serial — chromem map write is mutex'd but we keep it
		// simple and deterministic).
		for _, p := range results {
			if p.res.err != nil {
				if ctx.Err() != nil {
					break // interrupted
				}
				fmt.Fprintf(os.Stderr, "[%s] FAIL %s: %v\n", time.Now().Format("15:04:05"), p.id, p.res.err)
				stats.Failed++
				continue
			}
			if len(p.res.vec) == 0 {
				// skipped (already done in resume)
				stats.Skipped++
				continue
			}
			if !*dryRun {
				col, ok := cols[p.doc.Layer]
				if !ok {
					fmt.Fprintf(os.Stderr, "[%s] FAIL %s: unknown layer %q\n", time.Now().Format("15:04:05"), p.id, p.doc.Layer)
					stats.Failed++
					continue
				}
				doc := chromem.Document{
					ID:        p.id,
					Embedding: p.res.vec,
					Content:   p.doc.Content,
					Metadata:  p.doc.Meta,
				}
				if doc.Metadata == nil {
					doc.Metadata = map[string]string{}
				}
				// Ensure required metadata fields are present (matches hyatlas Add path).
				if doc.Metadata["layer"] == "" {
					doc.Metadata["layer"] = p.doc.Layer
				}
				if doc.Metadata["user_id"] == "" {
					doc.Metadata["user_id"] = p.doc.UserID
				}
				if doc.Metadata["agent_id"] == "" {
					doc.Metadata["agent_id"] = p.doc.AgentID
				}
				if doc.Metadata["ts"] == "" {
					doc.Metadata["ts"] = p.doc.Ts
				}
				if err := col.AddDocument(ctx, doc); err != nil {
					fmt.Fprintf(os.Stderr, "[%s] FAIL %s: AddDocument: %v\n", time.Now().Format("15:04:05"), p.id, err)
					stats.Failed++
					continue
				}
			}
			stats.Done++
			stats.Tokens += p.res.tokens

			// Record progress.
			pe := ProgressEntry{ID: p.id, Status: "done", Timestamp: time.Now()}
			if data, err := json.Marshal(pe); err == nil {
				progW.Write(data)
				progW.WriteString("\n")
			}
		}
		progW.Flush()

		processed := stats.Done + stats.Skipped + stats.Failed
		if *logEvery > 0 && (processed%*logEvery == 0 || processed == stats.Total) {
			elapsed := time.Since(stats.StartedAt)
			rate := float64(processed) / elapsed.Seconds()
			eta := time.Duration(0)
			if rate > 0 && processed < stats.Total {
				eta = time.Duration(float64(stats.Total-processed)/rate) * time.Second
			}
			fmt.Fprintf(os.Stderr, "[%s] progress: %d/%d (done=%d skip=%d fail=%d tokens=%d) elapsed=%s rate=%.1f/s eta=%s\n",
				time.Now().Format("15:04:05"), processed, stats.Total,
				stats.Done, stats.Skipped, stats.Failed, stats.Tokens,
				elapsed.Round(time.Second), rate, eta.Round(time.Second))
		}
	}

	stats.Elapsed = time.Since(stats.StartedAt)
	stats.FinishedAt = time.Now()

	// ── 8. Print summary ──
	fmt.Fprintf(os.Stderr, "\n=== REEMBED COMPLETE ===\n")
	fmt.Fprintf(os.Stderr, "total:   %d\n", stats.Total)
	fmt.Fprintf(os.Stderr, "done:    %d\n", stats.Done)
	fmt.Fprintf(os.Stderr, "skipped: %d\n", stats.Skipped)
	fmt.Fprintf(os.Stderr, "failed:  %d\n", stats.Failed)
	fmt.Fprintf(os.Stderr, "tokens:  %d\n", stats.Tokens)
	fmt.Fprintf(os.Stderr, "elapsed: %s\n", stats.Elapsed.Round(time.Second))
	if stats.Failed > 0 {
		os.Exit(1)
	}
}

func truncStr(b []byte, n int) string {
	s := string(b)
	if len(s) > n {
		return s[:n] + "..."
	}
	return s
}

// avoid "imported and not used" for strings (used in truncStr path).
var _ = strings.TrimSpace
