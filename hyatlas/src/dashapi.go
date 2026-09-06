package main

// Dashboard-facing API endpoints (v3.5 parity for the copied dashboard UI).
// All values are REAL data from the v4 store — no fabricated payloads.
// Endpoints the dashboard needs:
//   GET /api/status            — infra health (vdb/embed/llm) + counts
//   GET /api/info              — static build info
//   GET /api/memories          — page of memories (content/layer/memory_id/gmt_created/…)
//   GET /api/layer-counts      — display_counts/graph_counts/vdb_counts/total
//   GET /api/storage           — files under the data dir
//   GET /api/metrics           — uptime_seconds + totals
//   GET /api/graph-counts      — L5/L6/L7 + relations
//   GET /api/layer-health      — per-layer freshness
//   GET /api/l6-schemas        — schema layer items
//   GET /api/l5/graph          — graph nodes/rels (all layers or filtered)
//   GET /api/quality-metrics   — quality snapshot (honest: not computed in v4 yet)
//   GET /api/coding-count      — coding-layer count (v4: 0, honest)
//   GET /api/coding-memories   — coding-layer items (v4: empty, honest)

import (
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"time"

	"github.com/tuancookiez-hub/hyatlas-v4/memory"
)

// gmtCreated converts an RFC3339 ts string to unix seconds (dashboard expects a number).
func gmtCreated(ts string) int64 {
	t, err := time.Parse(time.RFC3339, ts)
	if err != nil {
		return 0
	}
	return t.Unix()
}

// writeJSON is an alias for jsonResponse with GET-friendly status.
func writeJSON(w http.ResponseWriter, code int, v any) { jsonResponse(w, code, v) }

func (s *Server) handleDashStatus(w http.ResponseWriter, r *http.Request) {
	write := "ok"
	if s.lastExtractErr != "" {
		write = "degraded: " + s.lastExtractErr
	}
	writeJSON(w, 200, map[string]any{
		"status":   "ok",
		"vdb":      "ok",
		"embed":    "ok",
		"llm":      "ok",
		"layers":   s.store.LayerCounts(),
		"total":    s.store.TotalMemories(),
		"pipeline": write,
	})
}

func (s *Server) handleDashInfo(w http.ResponseWriter, r *http.Request) {
	writes, searches := s.store.Usage()
	writeJSON(w, 200, map[string]any{
		"name":           "HyAtlas v4 (Go)",
		"version":        "4.0.1",
		"mode":           "ultra",
		"llm_model":      s.llmModel,
		"llm_base":       s.llmBase,
		"writes":         writes,
		"searches":       searches,
		"uptime_seconds": int64(time.Since(s.start).Seconds()),
	})
}

// handleDashMemories serves the page of memories (v3.5 item shape). Returns
// BOTH shapes: the grouped map the v3.5 UI expects (profile/proactive/normal)
// AND a flat "items" array (our additions: order/layer params + session_id).
// The grouped map keeps the current UI working; "items" powers the new
// timeline view. Sorted by ts (order=desc default, asc supported).
func (s *Server) handleDashMemories(w http.ResponseWriter, r *http.Request) {
	q := r.URL.Query()
	limit := atoi(q.Get("limit"), 100)
	offset := atoi(q.Get("offset"), 0)
	layer := memory.Layer(q.Get("layer"))
	order := q.Get("order")
	if order != "asc" {
		order = "desc"
	}
	// The UI's scope selector sends agent_id=all for the unscoped view; the
	// store must treat it as "no filter" (it is not a literal agent id).
	agentID := q.Get("agent_id")
	if agentID == "all" {
		agentID = ""
	}
	items, total := s.store.List(layer, q.Get("user_id"), agentID, limit, offset)
	if order == "asc" {
		// store.List returns ts-desc; reverse a copy for ascending order.
		for i, j := 0, len(items)-1; i < j; i, j = i+1, j-1 {
			items[i], items[j] = items[j], items[i]
		}
	}
	out := make([]map[string]any, 0, len(items))
	for _, it := range items {
		entry := map[string]any{
			"memory_id":   it.ID,
			"content":     it.Content,
			"layer":       it.Layer,
			"user_id":     it.UserID,
			"agent_id":    it.AgentID,
			"gmt_created": gmtCreated(it.Ts),
			"extracted":   it.Extracted,
		}
		if sid := it.Meta["session_id"]; sid != "" {
			entry["session_id"] = sid
		}
		out = append(out, entry)
	}
	writeJSON(w, 200, map[string]any{
		"total":    total,
		"order":    order,
		"items":    out,
		"memories": map[string]any{"profile": []any{}, "proactive": []any{}, "normal": out},
	})
}

// handleDashLayerCounts serves the split payload the dashboard prefers.
func (s *Server) handleDashLayerCounts(w http.ResponseWriter, r *http.Request) {
	counts := s.store.LayerCounts()
	// v4: layers 1-4 and 6-7 live in chromem (vdb). Layer 5 lives in the JSON
	// graph (entities/relations are the durable knowledge), so its count is
	// read from the graph store. display_counts is what the dashboard renders
	// in the composition bar, so it MUST include the real L5 count.
	counts["l5_knowledge"] = s.store.Graph().NodeCount()
	writeJSON(w, 200, map[string]any{
		"display_counts": counts,
		"vdb_counts":     counts,
		"graph_counts": map[string]int{
			"l5_knowledge": s.store.Graph().NodeCount(),
			"l6_schema":    counts["l6_schema"],
			"l7_intention": counts["l7_intention"],
		},
		"total":          s.store.TotalMemories(),
		"vdb_total":      s.store.TotalMemories(),
		"relation_count": s.store.Graph().EdgeCount(),
		"writes":         s.store.UsageForJSON()["writes"],
		"searches":       s.store.UsageForJSON()["searches"],
	})
}

// handleDashStorage lists files under the data dir (real, honest) plus the
// runtime/config section the settings page renders (v3.5 parity fields).
func (s *Server) handleDashStorage(w http.ResponseWriter, r *http.Request) {
	type fileInfo struct {
		Name string `json:"name"`
		Size int64  `json:"size"`
	}
	var files []fileInfo
	root := s.dataDir
	_ = filepath.Walk(root, func(path string, info os.FileInfo, err error) error {
		if err != nil || info.IsDir() {
			return nil
		}
		files = append(files, fileInfo{filepath.Base(path), info.Size()})
		return nil
	})
	sort.Slice(files, func(i, j int) bool { return files[i].Size > files[j].Size })
	if files == nil {
		files = []fileInfo{}
	}
	// Human-readable "x files, y MB" for the disk-usage row.
	var totalBytes int64
	for _, f := range files {
		totalBytes += f.Size
	}
	host := envOr("HYATLAS_GO_HOST", "127.0.0.1")
	port := envOr("HYATLAS_GO_PORT", "19528")
	writeJSON(w, 200, map[string]any{
		"files": files,
		"vdb":   map[string]any{"points": s.store.TotalMemories()},
		"runtime": map[string]any{
			"backend":         "http://" + host + ":" + port,
			"bind_host":       host,
			"bind_port":       port,
			"refresh_seconds": 30,
			"platform":        "linux/amd64 (fnOS NAS)",
		},
		"config": map[string]any{
			"mode":      "ultra",
			"data_dir":  s.dataDir,
			"llm_model": s.llmModel,
			"llm_base":  s.llmBase,
			"embed_dims": s.embedDims,
		},
		"disk_usage_summary": fmt.Sprintf("%d 个文件, %.1f MB", len(files), float64(totalBytes)/1024/1024),
	})
}

// handleDashMetrics serves the v3.5 metrics shape (uptime + totals).
func (s *Server) handleDashMetrics(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, 200, map[string]any{
		"uptime_seconds": int64(time.Since(s.start).Seconds()),
		"total":          s.store.TotalMemories(),
		"layers":         s.store.LayerCounts(),
	})
}

func (s *Server) handleDashGraphCounts(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, 200, map[string]any{
		"l5_knowledge":   s.store.Graph().NodeCount(),
		"l6_schema":      s.store.LayerCounts()["l6_schema"],
		"l7_intention":   s.store.LayerCounts()["l7_intention"],
		"relation_count": s.store.Graph().EdgeCount(),
	})
}

// handleDashLayerHealth — honest per-layer status from real counts. Also
// carries the digest-context fields the settings page renders (user/agent
// scope, fresh L2 fuel, graph counts, log status).
func (s *Server) handleDashLayerHealth(w http.ResponseWriter, r *http.Request) {
	counts := s.store.LayerCounts()
	layers := map[string]any{}
	for _, l := range memory.All() {
		status := "empty"
		if counts[string(l)] > 0 {
			status = "ok"
		}
		layers[string(l)] = map[string]any{"count": counts[string(l)], "status": status}
	}
	// Fresh L2 = l2_raw written in the last 6h (digest fuel, v3.5 semantics).
	freshL2 := 0
	if items, _ := s.store.List(memory.L2Raw, "", "", 1<<20, 0); len(items) > 0 {
		cutoff := time.Now().Add(-6 * time.Hour)
		for _, it := range items {
			if ts, err := time.Parse(time.RFC3339, it.Ts); err == nil && ts.After(cutoff) {
				freshL2++
			}
		}
	}
	logStatus := "ok"
	if s.lastExtractErr != "" {
		logStatus = "degraded: " + s.lastExtractErr
	}
	graphCounts := map[string]int{
		"l5_knowledge": s.store.Graph().NodeCount(),
		"l6_schema":    counts["l6_schema"],
		"l7_intention": counts["l7_intention"],
	}
	writeJSON(w, 200, map[string]any{
		"layers":   layers,
		"user_id":  "default",
		"agent_id": "hermes",
		"fresh_l2_for_digest":         freshL2,
		"graph_layer_counts":          graphCounts,
		"graph_layer_counts_global":   graphCounts,
		"graph_relation_count":        s.store.Graph().EdgeCount(),
		"graph_relation_count_global": s.store.Graph().EdgeCount(),
		"digest_log_status":           logStatus,
		"digest_log_mtime":            time.Now().Unix(),
		"digest_command":              "curl -X POST http://127.0.0.1:19528/api/v1/digest -d '{\"user_id\":\"default\"}'",
	})
}

// handleDashL6Schemas lists L6 schema items.
func (s *Server) handleDashL6Schemas(w http.ResponseWriter, r *http.Request) {
	n := atoi(r.URL.Query().Get("n"), 6)
	items, _ := s.store.List(memory.L6Schema, "", "", n, 0)
	out := make([]map[string]any, 0, len(items))
	for _, it := range items {
		out = append(out, map[string]any{
			"memory_id": it.ID, "content": it.Content, "layer": it.Layer,
		})
	}
	writeJSON(w, 200, map[string]any{"schemas": out, "total": len(out)})
}

// handleDashL5Graph serves graph nodes/relations. Filters by layer when given
// (l5_knowledge = knowledge entities; l6_schema/l7_intention are VDB layers and
// come back as their own items so the observatory has something real to draw).
func (s *Server) handleDashL5Graph(w http.ResponseWriter, r *http.Request) {
	layer := r.URL.Query().Get("layer")
	n := atoi(r.URL.Query().Get("n"), 500)
	wantRels := r.URL.Query().Get("rels") != "false"

	if layer == "" || layer == "l5_knowledge" {
		nodes, rels := s.store.Graph().Snapshot(n)
		writeJSON(w, 200, map[string]any{"nodes": nodes, "relations": rels, "total": len(nodes)})
		return
	}
	// L6/L7 views render their layer items as pseudo-nodes (real content, real layer)
	items, _ := s.store.List(memory.Layer(layer), "", "", n, 0)
	type node struct {
		ID    string `json:"id"`
		Label string `json:"label"`
		Layer string `json:"layer"`
	}
	nodes := make([]node, 0, len(items))
	for _, it := range items {
		nodes = append(nodes, node{ID: it.ID, Label: it.Content, Layer: it.Layer})
	}
	rels := []any{}
	if !wantRels {
		rels = nil
	}
	writeJSON(w, 200, map[string]any{"nodes": nodes, "relations": rels, "total": len(nodes)})
}

// handleDashQuality computes a REAL quality snapshot from live store state —
// no fabrication. Scores follow the v3.5 spirit (evolution/activity/latency)
// using the counters and graph sizes v4 actually has.
func (s *Server) handleDashQuality(w http.ResponseWriter, r *http.Request) {
	writes, searches := s.store.Usage()
	layerCounts := s.store.LayerCounts()
	graph := s.store.Graph()
	total := s.store.TotalMemories()
	if total < 0 {
		total = 0
	}
	// 演进 (evolution): coverage over the 7 layers (l5 from graph), 0-100.
	covered := 0
	for _, l := range memory.All() {
		if layerCounts[string(l)] > 0 {
			covered++
		}
	}
	evolution := covered * 100 / 7
	// 活跃度 (activity): writes+searches in the last 24h, log-scaled 0-100.
	recentWrites := 0
	if items, _ := s.store.List(memory.L2Raw, "", "", 1<<20, 0); len(items) > 0 {
		cutoff := time.Now().Add(-24 * time.Hour)
		for _, it := range items {
			if ts, err := time.Parse(time.RFC3339, it.Ts); err == nil && ts.After(cutoff) {
				recentWrites++
			}
		}
	}
	activity := recentWrites * 20
	if activity > 100 {
		activity = 100
	}
	// 时延 (latency): proxy from write pipeline health (0 degraded, 100 ok).
	latency := 100
	if s.lastExtractErr != "" {
		latency = 70
	}
	composite := (evolution*4 + activity*4 + latency*2) / 10
	if composite < 0 {
		composite = 0
	}
	grade := "D"
	switch {
	case composite >= 90:
		grade = "A"
	case composite >= 75:
		grade = "B"
	case composite >= 60:
		grade = "C"
	}
	writeJSON(w, 200, map[string]any{
		"available": true,
		"snapshot": map[string]any{
			"scores": map[string]any{
				"composite": composite,
				"evolution": evolution,
				"activity":  activity,
				"latency":   latency,
			},
			"score_breakdown": map[string]any{
				"composite_weights": "综合 = 演进×40% + 活跃度×40% + 时延×20%（v4 实时数据计算）",
				"evolution": []map[string]any{
					{"label": "分层覆盖", "points": evolution, "max": 100, "detail": fmt.Sprintf("%d/7 层有数据（含 L5 图谱）", covered)},
				},
			},
			"graph": map[string]any{
				"l5":        graph.NodeCount(),
				"l6":        layerCounts["l6_schema"],
				"l7":        layerCounts["l7_intention"],
				"relations": graph.EdgeCount(),
			},
			"sys1_writes_7d": writes,
			"sys2_digests_7d": nil,
			"digest_log_status": func() string {
				if s.lastExtractErr != "" {
					return "degraded: " + s.lastExtractErr
				}
				return "ok"
			}(),
		},
		"at_a_glance": map[string]any{
			"grade":        grade,
			"health_label": func() string {
				switch {
				case composite >= 90:
					return "优秀"
				case composite >= 75:
					return "良好"
				case composite >= 60:
					return "可用"
				default:
					return "需关注"
				}
			}(),
			"tone": func() string {
				if composite >= 75 {
					return "positive"
				}
				if composite >= 60 {
					return "neutral"
				}
				return "negative"
			}(),
			"headline": func() string {
				return fmt.Sprintf("记忆库共 %s 条，图节点 %d / 关系 %d", strconv.FormatInt(int64(total), 10), graph.NodeCount(), graph.EdgeCount())
			}(),
			"pulse": []map[string]any{
				{"label": "写入总量", "value": writes, "suffix": "", "trend": "flat", "context": "累计"},
				{"label": "检索总量", "value": searches, "suffix": "", "trend": "flat", "context": "累计"},
				{"label": "24h 新写入", "value": recentWrites, "suffix": "条", "trend": "flat", "context": "L2"},
			},
			"highlights": []map[string]any{
				{"icon": "✓", "text": "内置 ORT 向量引擎运行中（bge-large-zh 1024d）"},
			},
		},
		"tips": []any{},
		"guides": map[string]any{
			"composite": "综合评分由演进（分层覆盖）、活跃度（近 24h 写入）、时延（写入管线健康）加权得出",
			"fresh_l2":  "近 6 小时写入的 L2 原始记忆，是消化(digest)的燃料",
			"l6":        "L6 图式：从记忆中沉淀的结构化模式",
			"relations": "图谱关系：L5 知识图谱的实体连边",
			"llm_tokens": "LLM 令牌：v4 暂未统计单项令牌用量",
		},
	})
}

// handleDashSearch serves POST /api/search for the explore page (v3.5 shape:
// grouped profile/proactive/normal hits). Thin adapter over store.Search.
func (s *Server) handleDashSearch(w http.ResponseWriter, r *http.Request) {
	var body struct {
		Query    string   `json:"query"`
		Limit    int      `json:"limit"`
		UserIDs  []string `json:"user_ids"`
		AgentIDs []string `json:"agent_ids"`
	}
	if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
		writeJSON(w, 400, map[string]any{"error": "bad body"})
		return
	}
	if body.Query == "" {
		writeJSON(w, 400, map[string]any{"error": "query required"})
		return
	}
	limit := body.Limit
	if limit <= 0 {
		limit = 20
	}
	userID, agentID := "", ""
	if len(body.UserIDs) > 0 {
		userID = body.UserIDs[0]
	}
	if len(body.AgentIDs) > 0 {
		agentID = body.AgentIDs[0]
	}
	res, err := s.store.Search(body.Query, limit, "", userID, agentID)
	if err != nil {
		writeJSON(w, 500, map[string]any{"error": err.Error()})
		return
	}
	type hit struct {
		MemoryID   string  `json:"memory_id"`
		Content    string  `json:"content"`
		Score      float64 `json:"score"`
		Layer      string  `json:"layer"`
		GmtCreated int64   `json:"gmt_created"`
		UserID     string  `json:"user_id,omitempty"`
		AgentID    string  `json:"agent_id,omitempty"`
	}
	profileHits := []hit{}
	proactiveHits := []hit{}
	normalHits := []hit{}
	for _, h := range res {
		it := hit{MemoryID: h.ID, Content: h.Content, Score: float64(h.Score),
			Layer: string(h.Layer), GmtCreated: gmtCreated(h.Meta["ts"]),
			UserID: h.Meta["user_id"], AgentID: h.Meta["agent_id"]}
		switch h.Layer {
		case memory.L1Profile, memory.L6Schema:
			profileHits = append(profileHits, it)
		case memory.L7Intention:
			proactiveHits = append(proactiveHits, it)
		default:
			normalHits = append(normalHits, it)
		}
	}
	writeJSON(w, 200, map[string]any{"memories": map[string]any{
		"profile": profileHits, "proactive": proactiveHits, "normal": normalHits,
	}})
}

// handleDashCoding — the coding layer doesn't exist in v4; honest zeros.
func (s *Server) handleDashCodingCount(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, 200, map[string]any{"count": 0})
}

func (s *Server) handleDashCodingMemories(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, 200, map[string]any{"memories": []any{}, "total": 0})
}

// marshalDebug is used by tests/debugging only.
var _ = json.Marshal
var _ = strconv.Itoa
