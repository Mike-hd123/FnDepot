package delaytest

import (
	"encoding/json"
	"fluxor/internal/httpx"
	"net/http"
	"strconv"
	"strings"
	"time"
)

// HandleDelayTestGoogle 测试 Google 延迟
func HandleDelayTestGoogle(w http.ResponseWriter, r *http.Request) {
	handleDelayTestCommon(w, r, "https://www.gstatic.com/generate_204")
}

// HandleDelayTestYouTube 测试 YouTube 延迟
func HandleDelayTestYouTube(w http.ResponseWriter, r *http.Request) {
	handleDelayTestCommon(w, r, "https://www.youtube.com")
}

// HandleDelayTestGitHub 测试 GitHub 延迟
func HandleDelayTestGitHub(w http.ResponseWriter, r *http.Request) {
	handleDelayTestCommon(w, r, "https://github.com")
}

// HandleDelayTestBaidu 测试 Baidu 延迟
func HandleDelayTestBaidu(w http.ResponseWriter, r *http.Request) {
	handleDelayTestCommon(w, r, "https://www.baidu.com")
}

// HandleDelayTestBilibili 测试 Bilibili 延迟
func HandleDelayTestBilibili(w http.ResponseWriter, r *http.Request) {
	handleDelayTestCommon(w, r, "https://www.bilibili.com")
}

// HandleDelayTestCustom 测试自定义 URL 延迟
func HandleDelayTestCustom(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		httpx.WriteJSONError(w, http.StatusMethodNotAllowed, "Method Not Allowed")
		return
	}
	customURL := r.URL.Query().Get("url")
	if customURL == "" {
		httpx.WriteJSONError(w, http.StatusBadRequest, "缺少 url 参数")
		return
	}
	// 简单校验是否以 http:// 或 https:// 开头
	if !strings.HasPrefix(customURL, "http://") && !strings.HasPrefix(customURL, "https://") {
		httpx.WriteJSONError(w, http.StatusBadRequest, "url 必须以 http:// 或 https:// 开头")
		return
	}
	timeoutMs := parseDelayTimeout(r)
	timeout := time.Duration(timeoutMs) * time.Millisecond
	delay, err := testDelayThroughProxy(r.Context(), customURL, timeout)
	if err != nil {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]interface{}{"delay": nil, "error": err.Error()})
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]int{"delay": delay})
}

// parseDelayTimeout 解析 timeout 查询参数（毫秒）。
//
// 必须设上限：此前只校验 val > 0，?timeout=999999999 会让请求长期占用
// 一个 goroutine 与一条代理连接。
func parseDelayTimeout(r *http.Request) int {
	const (
		defaultTimeoutMs = 5000
		maxTimeoutMs     = 30000
	)
	t := r.URL.Query().Get("timeout")
	if t == "" {
		return defaultTimeoutMs
	}
	val, err := strconv.Atoi(t)
	if err != nil || val <= 0 {
		return defaultTimeoutMs
	}
	if val > maxTimeoutMs {
		return maxTimeoutMs
	}
	return val
}

// 公共处理函数
func handleDelayTestCommon(w http.ResponseWriter, r *http.Request, targetURL string) {
	if r.Method != http.MethodGet {
		httpx.WriteJSONError(w, http.StatusMethodNotAllowed, "Method Not Allowed")
		return
	}
	// 读取超时参数，默认5000ms
	timeoutMs := parseDelayTimeout(r)
	timeout := time.Duration(timeoutMs) * time.Millisecond
	delay, err := testDelayThroughProxy(r.Context(), targetURL, timeout)
	if err != nil {
		// 超时或错误，返回 delay=null 或 -1，前端显示超时
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]interface{}{"delay": nil, "error": err.Error()})
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]int{"delay": delay})
}
