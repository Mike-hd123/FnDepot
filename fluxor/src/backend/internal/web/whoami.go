package web

import (
	"encoding/json"
	"net/http"
)

// HandleWhoAmI 返回当前用户信息（从 X-Trim-Username 请求头读取）
func HandleWhoAmI(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}
	username := r.Header.Get("X-Trim-Username")
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"username": username})
}
