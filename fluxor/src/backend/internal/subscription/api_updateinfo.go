package subscription

import (
	"encoding/json"
	"fluxor/internal/config"
	"fluxor/internal/httpx"
	"log"
	"net/http"
	"net/url"
	"strings"
)

// HandleUpdateSubscriptionInfo 用于前端融合模式手动更新后持久化单个订阅元数据
func HandleUpdateSubscriptionInfo(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}
	path := strings.TrimPrefix(r.URL.Path, config.BaseURL+"/subscribe/update-info/")
	if path == "" {
		httpx.WriteJSONError(w, http.StatusBadRequest, "缺少订阅名称")
		return
	}
	name, err := url.QueryUnescape(path)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadRequest, "无效的订阅名称")
		return
	}

	var payload struct {
		Upload    int64  `json:"Upload"`
		Download  int64  `json:"Download"`
		Total     int64  `json:"Total"`
		Expire    int64  `json:"Expire"`
		UpdatedAt string `json:"updatedAt"`
	}
	if err := json.NewDecoder(r.Body).Decode(&payload); err != nil {
		httpx.WriteJSONError(w, http.StatusBadRequest, "无效的请求体: "+err.Error())
		return
	}

	config.Mu.Lock()
	found := false
	for i := range config.Current.Subscriptions {
		if config.Current.Subscriptions[i].Name == name {
			subInfo := map[string]interface{}{
				"upload":   payload.Upload,
				"download": payload.Download,
				"total":    payload.Total,
				"expire":   payload.Expire,
			}
			config.Current.Subscriptions[i].UpdatedAt = payload.UpdatedAt
			config.Current.Subscriptions[i].SubscriptionInfo = subInfo
			found = true
			break
		}
	}
	config.Mu.Unlock()

	if !found {
		httpx.WriteJSONError(w, http.StatusNotFound, "订阅不存在")
		return
	}

	if err := config.SaveSubscribeConfig(); err != nil {
		log.Printf("保存配置失败: %v", err)
		httpx.WriteJSONError(w, http.StatusInternalServerError, "保存失败")
		return
	}
	httpx.RespondJSON(w, http.StatusOK, map[string]string{"status": "ok", "message": "订阅信息已更新"})
}
