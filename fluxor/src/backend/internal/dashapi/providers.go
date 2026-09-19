package dashapi

import (
	"fluxor/internal/config"
	"fluxor/internal/core"
	"fluxor/internal/httpx"
	"io"
	"net/http"
	"strings"
)

// HandleProvidersProxiesAll 获取所有订阅的代理信息（含节点历史、代理组）
// 对应内核 GET /providers/proxies
func HandleProvidersProxiesAll(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}
	resp, err := core.CoreRequest("GET", "/providers/proxies", nil)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "获取订阅代理信息失败: "+err.Error())
		return
	}
	defer resp.Body.Close()
	w.Header().Set("Content-Type", "application/json")
	io.Copy(w, resp.Body)
}

// HandleProviderProxies 获取指定订阅的代理信息（含流量/有效期）
// GET /providers/proxies/{encodedName} 和 /providers/proxies/{provider}/{proxy}/healthcheck
// PUT /providers/proxies/{encodedName} 更新订阅
func HandleProviderProxies(w http.ResponseWriter, r *http.Request) {
	// 提取路径 /providers/proxies/{name}
	path := r.URL.EscapedPath()
	trimmed := strings.TrimPrefix(path, config.BaseURL+"/providers/proxies/")
	if trimmed == "" {
		httpx.WriteJSONError(w, http.StatusBadRequest, "缺少代理名称")
		return
	}
	// 该片段会被拼进内核路径，先排除穿越形态
	if !validateCorePathSuffix(trimmed) {
		httpx.WriteJSONError(w, http.StatusBadRequest, "无效的代理名称")
		return
	}
	targetPath := "/providers/proxies/" + trimmed

	// 拼接查询参数（用于 healthcheck）
	if r.URL.RawQuery != "" {
		targetPath += "?" + r.URL.RawQuery
	}

	var resp *http.Response
	var err error

	switch r.Method {
	case http.MethodGet:
		resp, err = core.CoreRequest("GET", targetPath, nil)
	case http.MethodPut:
		resp, err = core.CoreRequest("PUT", targetPath, r.Body)
	default:
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}

	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "请求失败: "+err.Error())
		return
	}
	defer resp.Body.Close()

	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(resp.StatusCode)
	io.Copy(w, resp.Body)
}
