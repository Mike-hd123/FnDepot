package web

import (
	"encoding/json"
	"fluxor/internal/buildinfo"
	"net/http"
)

// HandleAppVersion 返回 Fluxor 自身的版本号（编译期注入）。
//
// 前端「关于」页展示与更新检查均以此为准，取代此前由 Vite define 注入的
// __APP_VERSION__。版本在编译时经 -ldflags 写入 buildinfo.Version。
func HandleAppVersion(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"version": buildinfo.Name()})
}
