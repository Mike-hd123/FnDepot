package dashapi

import (
	"fluxor/internal/core"
	"fluxor/internal/httpx"
	"io"
	"net/http"
)

// HandleUpgrade 更新内核（POST /upgrade）
func HandleUpgrade(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		httpx.WriteJSONError(w, http.StatusMethodNotAllowed, "Method Not Allowed")
		return
	}
	// 构建目标路径，包含查询参数
	targetPath := "/upgrade"
	if r.URL.RawQuery != "" {
		targetPath += "?" + r.URL.RawQuery
	}
	resp, err := core.CoreRequest("POST", targetPath, nil)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "请求内核失败: "+err.Error())
		return
	}
	defer resp.Body.Close()

	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(resp.StatusCode)
	io.Copy(w, resp.Body)
}
