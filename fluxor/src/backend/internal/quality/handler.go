package quality

import (
	"encoding/json"
	"fluxor/internal/config"
	"fluxor/internal/core"
	"fluxor/internal/httpx"
	"math"
	"net/http"
)

// HandleQualityScores 返回所有节点的质量分数（从内核获取历史数据计算）
func HandleQualityScores(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		httpx.WriteJSONError(w, http.StatusMethodNotAllowed, "Method Not Allowed")
		return
	}

	// 获取当前订阅模式
	config.Mu.RLock()
	mode := config.Current.Mode
	config.Mu.RUnlock()

	// 统一数据结构：nodeName -> history
	nodeHistories := make(map[string][]struct {
		Time  string `json:"time"`
		Delay int    `json:"delay"`
	})

	if mode == "merge" {
		// 融合模式：从 /providers/proxies 获取
		resp, err := core.CoreRequest("GET", "/providers/proxies", nil)
		if err != nil {
			httpx.WriteJSONError(w, http.StatusBadGateway, "获取订阅代理数据失败: "+err.Error())
			return
		}
		defer resp.Body.Close()
		if resp.StatusCode != http.StatusOK {
			httpx.WriteJSONError(w, resp.StatusCode, "内核返回错误")
			return
		}

		var providersData struct {
			Providers map[string]struct {
				Proxies []struct {
					Name    string `json:"name"`
					History []struct {
						Time  string `json:"time"`
						Delay int    `json:"delay"`
					} `json:"history"`
				} `json:"proxies"`
			} `json:"providers"`
		}
		if err := json.NewDecoder(resp.Body).Decode(&providersData); err != nil {
			httpx.WriteJSONError(w, http.StatusInternalServerError, "解析内核数据失败: "+err.Error())
			return
		}

		for _, provider := range providersData.Providers {
			for _, node := range provider.Proxies {
				if node.Name == "" || len(node.History) == 0 {
					continue
				}
				nodeHistories[node.Name] = node.History
			}
		}
	} else {
		// 切换模式：从 /proxies 获取
		resp, err := core.CoreRequest("GET", "/proxies", nil)
		if err != nil {
			httpx.WriteJSONError(w, http.StatusBadGateway, "获取代理数据失败: "+err.Error())
			return
		}
		defer resp.Body.Close()
		if resp.StatusCode != http.StatusOK {
			httpx.WriteJSONError(w, resp.StatusCode, "内核返回错误")
			return
		}

		var proxiesData struct {
			Proxies map[string]struct {
				History []struct {
					Time  string `json:"time"`
					Delay int    `json:"delay"`
				} `json:"history"`
			} `json:"proxies"`
		}
		if err := json.NewDecoder(resp.Body).Decode(&proxiesData); err != nil {
			httpx.WriteJSONError(w, http.StatusInternalServerError, "解析内核数据失败: "+err.Error())
			return
		}
		for name, proxy := range proxiesData.Proxies {
			if len(proxy.History) > 0 {
				nodeHistories[name] = proxy.History
			}
		}
	}

	// 计算每个节点的质量分数
	scores := make(map[string]NodeQualityScore)
	for name, history := range nodeHistories {
		latencies := make([]int, 0, len(history))
		histEntries := make([]NodeHistoryEntry, 0, len(history))
		for _, h := range history {
			latencies = append(latencies, h.Delay)
			histEntries = append(histEntries, NodeHistoryEntry{Latency: h.Delay})
		}
		if len(latencies) == 0 {
			scores[name] = NodeQualityScore{Score: 0, LatencyScore: 0, Stability: 0, SuccessRate: 0}
			continue
		}
		latScore := calcLatencyScore(latencies)
		stabScore := calcStabilityScore(latencies)
		succScore := calcSuccessRateScore(histEntries)
		totalWeight := weightLatency + weightStability + weightSuccessRate
		score := float64(latScore)*float64(weightLatency)/float64(totalWeight) +
			float64(stabScore)*float64(weightStability)/float64(totalWeight) +
			float64(succScore)*float64(weightSuccessRate)/float64(totalWeight)
		scores[name] = NodeQualityScore{
			Score:        int(math.Round(score)),
			LatencyScore: latScore,
			Stability:    stabScore,
			SuccessRate:  succScore,
		}
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(scores)
}
