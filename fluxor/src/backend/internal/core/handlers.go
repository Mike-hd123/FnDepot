package core

import (
	"encoding/json"
	"fluxor/internal/httpx"
	"net/http"
)

// HandleCoreStatus 返回内核运行状态
func HandleCoreStatus(w http.ResponseWriter, r *http.Request) {
	running := IsCoreRunning()
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]interface{}{"running": running})
}

// HandleCoreStart 启动内核
func HandleCoreStart(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}
	if err := StartCore(); err != nil {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{"status": "error", "message": err.Error()})
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "ok", "message": "内核已启动"})
}

// HandleCoreStop 停止内核
func HandleCoreStop(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}
	if err := StopCore(); err != nil {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{"status": "error", "message": err.Error()})
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "ok", "message": "内核已停止"})
}

// HandleCoreRestart 热重启内核（通过重载配置文件）
func HandleCoreRestart(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		httpx.WriteJSONError(w, http.StatusMethodNotAllowed, "Method Not Allowed")
		return
	}
	if err := ReloadCore(); err != nil {
		if CoreLogger != nil {
			CoreLogger.Printf("[RESTART][ERROR] 内核热重启失败: %v\n", err)
		}
		httpx.WriteJSONError(w, http.StatusInternalServerError, "内核热重启失败: "+err.Error())
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "ok", "message": "内核已热重启（重载配置）"})
}
