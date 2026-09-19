package dashapi

import (
	"fluxor/internal/core"
	"fluxor/internal/httpx"
	"io"
	"net/http"
)

// HandleVersion 返回内核版本信息（代理 /version）
func HandleVersion(w http.ResponseWriter, _ *http.Request) {
	resp, err := core.CoreRequest("GET", "/version", nil)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "无法获取内核版本: "+err.Error())
		return
	}
	defer resp.Body.Close()
	w.Header().Set("Content-Type", "application/json")
	io.Copy(w, resp.Body)
}
