package quality

import (
	"math"
)

// 计算延迟评分（0-100）
func calcLatencyScore(latencies []int) int {
	var valid []int
	for _, l := range latencies {
		if l > 0 {
			valid = append(valid, l)
		}
	}
	if len(valid) == 0 {
		return 0
	}
	sum := 0
	for _, l := range valid {
		sum += l
	}
	avg := float64(sum) / float64(len(valid))

	// 对数归一化：50ms=100, 500ms=50, 5000ms=0
	if avg <= 50 {
		return 100
	}
	if avg >= 5000 {
		return 0
	}
	minLog := math.Log(50)
	maxLog := math.Log(5000)
	currentLog := math.Log(avg)
	score := 100 * (1 - (currentLog-minLog)/(maxLog-minLog))
	return int(math.Round(score))
}

// 计算稳定性评分（0-100）
func calcStabilityScore(latencies []int) int {
	var valid []int
	for _, l := range latencies {
		if l > 0 {
			valid = append(valid, l)
		}
	}
	if len(valid) == 0 {
		return 0
	}
	if len(valid) < 2 {
		return 50 // 单样本给中性分
	}
	mean := 0.0
	for _, l := range valid {
		mean += float64(l)
	}
	mean /= float64(len(valid))
	variance := 0.0
	for _, l := range valid {
		diff := float64(l) - mean
		variance += diff * diff
	}
	variance /= float64(len(valid))
	stdDev := math.Sqrt(variance)
	cv := stdDev / mean // 变异系数
	if cv <= 0.1 {
		return 100
	}
	if cv >= 0.5 {
		return 0
	}
	return int(math.Round(100 * (1 - (cv-0.1)/0.4)))
}

// 计算成功率评分（0-100）
func calcSuccessRateScore(histories []NodeHistoryEntry) int {
	if len(histories) == 0 {
		return 0
	}
	success := 0
	for _, h := range histories {
		if h.Latency > 0 {
			success++
		}
	}
	return int(math.Round(float64(success) / float64(len(histories)) * 100))
}
