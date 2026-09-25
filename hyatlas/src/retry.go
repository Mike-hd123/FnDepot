package main

import (
	"log"
	"sync"
)

// retryWorker is a minimal in-process retry queue for failed L2 extractions.
//
// Background (2026-09-08 postmortem): handleAdd fires a fire-and-forget
// goroutine per memory; when the LLM call fails (timeout / 5xx / unparseable
// JSON) the item stays extracted=false forever, and handleReprocess only ever
// looked at the newest 200 raw items, so anything older was stranded. This
// worker adds bounded, serialized retries with exponential backoff so
// transient gateway stalls self-heal.
type retryWorker struct {
	ch   chan string // ids of L2 items to retry (capacity 256)
	mu   sync.Mutex
	sema chan struct{} // capacity 1: only one extraction at a time
}

func newRetryWorker() *retryWorker {
	return &retryWorker{
		ch:   make(chan string, 256),
		sema: make(chan struct{}, 1),
	}
}

// Enqueue schedules one memory id for retry. Drops the request silently when
// the queue is full — the item simply remains extracted=false and will be
// picked up by a later /api/v1/reprocess call (same as before).
func (w *retryWorker) Enqueue(id string) {
	if w == nil || id == "" {
		return
	}
	select {
	case w.ch <- id:
	default:
		log.Printf("retryWorker: queue full, dropping retry for %s (needs manual reprocess)", id)
	}
}

// Run consumes retry requests forever. Serialized via sema so we never stack
// concurrent LLM extraction loops (mirrors reprocess behavior; keeps gateway
// load flat).
func (w *retryWorker) Run(s *Server) {
	for id := range w.ch {
		w.sema <- struct{}{}
		go func(id string) {
			defer func() { <-w.sema }()
			doc, ok := s.store.GetDoc(id)
			if !ok || doc.Extracted {
				return // deleted, or already extracted by the original goroutine
			}
			if s.extractItem(doc) {
				log.Printf("retryWorker: %s extracted via retry queue", id)
			} else {
				log.Printf("retryWorker: %s gave up after retries (stays pending)", id)
			}
		}(id)
	}
}
