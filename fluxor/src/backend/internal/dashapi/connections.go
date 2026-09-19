package dashapi

import (
	"fluxor/internal/config"
	"fluxor/internal/core"
	"fluxor/internal/httpx"
	"net/http"
	"strings"
)

// HandleConnectionsClose 关闭单个连接或所有连接（DELETE /connections 或 /connections/{id}）
func HandleConnectionsClose(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodDelete {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}

	// 从路径中提取 ID（支持 /connections 和 /connections/xxx）
	path := strings.TrimPrefix(r.URL.Path, config.BaseURL+"/connections")
	var id string
	if path != "" && path != "/" {
		id = strings.TrimPrefix(path, "/")
	}

	// id 会被拼进内核路径，必须先排除穿越形态（如 %2e%2e%2f 会被解码为 ".."）
	if id != "" && !validateCorePathSuffix(id) {
		httpx.WriteJSONError(w, http.StatusBadRequest, "无效的连接 ID")
		return
	}

	var targetPath string
	if id != "" {
		targetPath = "/connections/" + id
	} else {
		targetPath = "/connections"
	}

	resp, err := core.CoreRequest("DELETE", targetPath, nil)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "关闭连接失败: "+err.Error())
		return
	}
	defer resp.Body.Close()
	w.WriteHeader(resp.StatusCode)
}
