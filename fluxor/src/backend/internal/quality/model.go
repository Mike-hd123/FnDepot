package quality

// NodeHistoryEntry 单次测速历史记录（仅用于计算）
type NodeHistoryEntry struct {
	Latency int `json:"latency"` // 毫秒，-1 表示超时/失败
}

// NodeQualityScore 节点质量分数
type NodeQualityScore struct {
	Score        int `json:"score"`        // 综合评分 0-100
	LatencyScore int `json:"latencyScore"` // 延迟评分
	Stability    int `json:"stability"`    // 稳定性评分
	SuccessRate  int `json:"successRate"`  // 成功率评分
}

// 评分权重（可配置，暂时固定）
const (
	weightLatency     = 50
	weightStability   = 30
	weightSuccessRate = 20
)
