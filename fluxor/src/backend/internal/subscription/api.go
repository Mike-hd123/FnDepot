package subscription

import (
	"encoding/json"
	"fluxor/internal/config"
	"fluxor/internal/httpx"
	"log"
	"net/http"
)

// HandleSubscribeConfigAPI 处理 GET /subscribe/config 和 POST /subscribe/config
func HandleSubscribeConfigAPI(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		config.Mu.RLock()
		defer config.Mu.RUnlock()

		w.Header().Set("Content-Type", "application/json")
		if err := json.NewEncoder(w).Encode(config.Current); err != nil {
			log.Printf("编码订阅配置失败: %v", err)
		}

	case http.MethodPost:
		var newConfig config.SubscribeConfig
		if err := json.NewDecoder(r.Body).Decode(&newConfig); err != nil {
			httpx.WriteJSONError(w, http.StatusBadRequest, "无效的请求格式: "+err.Error())
			return
		}
		if newConfig.MetaBackendURL != "" && !httpx.BackendURLRegex.MatchString(newConfig.MetaBackendURL) {
			httpx.WriteJSONError(w, http.StatusBadRequest, "外部面板后端地址格式不正确")
			return
		}
		// 订阅名会用作节点文件名与 provider 键，必须在此拦截非法字符
		if err := config.ValidateSubscriptionNames(newConfig.Subscriptions); err != nil {
			httpx.WriteJSONError(w, http.StatusBadRequest, err.Error())
			return
		}
		config.Mu.Lock()
		config.Current = newConfig
		config.Mu.Unlock()

		if err := config.SaveSubscribeConfig(); err != nil {
			log.Printf("保存订阅配置失败: %v", err)
			httpx.WriteJSONError(w, http.StatusInternalServerError, "保存配置失败: "+err.Error())
			return
		}

		// 重置定时器（先停止再启动）
		StopAllTimers()
		StartAllTimers()

		httpx.RespondJSON(w, http.StatusOK, map[string]string{
			"status":  "ok",
			"message": "配置已保存",
		})

	default:
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
	}
}
