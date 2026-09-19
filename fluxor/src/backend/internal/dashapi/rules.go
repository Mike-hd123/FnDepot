package dashapi

import (
	"fluxor/internal/config"
	"fluxor/internal/core"
	"fluxor/internal/httpx"
	"io"
	"net/http"
	"strings"
)

// HandleRules 获取所有规则（代理 GET /rules）
func HandleRules(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}
	resp, err := core.CoreRequest("GET", "/rules", nil)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "获取规则列表失败: "+err.Error())
		return
	}
	defer resp.Body.Close()
	w.Header().Set("Content-Type", "application/json")
	io.Copy(w, resp.Body)
}

// HandleRuleProviders 获取规则提供商（代理 GET /providers/rules）
func HandleRuleProviders(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}
	resp, err := core.CoreRequest("GET", "/providers/rules", nil)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "获取规则提供商失败: "+err.Error())
		return
	}
	defer resp.Body.Close()
	w.Header().Set("Content-Type", "application/json")
	io.Copy(w, resp.Body)
}

// HandleUpdateRuleProvider 更新规则提供商（PUT /providers/rules/{name}）
func HandleUpdateRuleProvider(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPut {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}
	path := r.URL.EscapedPath()
	// 使用 baseURL + "/providers/rules/" 作为前缀
	trimmed := strings.TrimPrefix(path, config.BaseURL+"/providers/rules/")
	if !validateSinglePathSegment(trimmed) {
		httpx.WriteJSONError(w, http.StatusBadRequest, "无效的提供商名称")
		return
	}
	providerName := trimmed
	targetPath := "/providers/rules/" + providerName
	resp, err := core.CoreRequest("PUT", targetPath, r.Body)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "更新规则提供商失败: "+err.Error())
		return
	}
	defer resp.Body.Close()
	w.WriteHeader(resp.StatusCode)
	io.Copy(w, resp.Body)
}

// HandleRulesDisable 禁用/启用规则（代理 PATCH /rules/disable）
func HandleRulesDisable(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPatch {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}
	resp, err := core.CoreRequest("PATCH", "/rules/disable", r.Body)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "规则禁用/启用失败: "+err.Error())
		return
	}
	defer resp.Body.Close()
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(resp.StatusCode)
	io.Copy(w, resp.Body)
}
