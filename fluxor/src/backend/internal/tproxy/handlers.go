package tproxy

import (
	"encoding/json"
	"fluxor/internal/config"
	"fluxor/internal/httpx"
	"log"
	"net/http"
)

// tproxyPort 读取当前生效的 TProxy 端口。
func tproxyPort() int {
	config.Mu.RLock()
	defer config.Mu.RUnlock()
	return config.Current.TproxyPort
}

// HandleTproxyState 处理 TProxy 开关状态：GET 查询，POST 切换。
func HandleTproxyState(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		httpx.RespondJSON(w, http.StatusOK, map[string]bool{"enabled": GetTproxyState()})

	case http.MethodPost:
		var req struct{ Enable bool }
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			httpx.WriteJSONError(w, http.StatusBadRequest, "无效的请求格式")
			return
		}

		if !req.Enable {
			DisableTProxyRules()
			SetTproxyEnabled(false)
			httpx.RespondJSON(w, http.StatusOK, map[string]bool{"enabled": false})
			return
		}

		port := tproxyPort()
		if port <= 0 {
			// 端口为 0 时内核不会监听，规则必然无法生效；明确拒绝而不是
			// 记一条日志后仍回报 enabled=true（那会让面板显示与实际不符）
			httpx.WriteJSONError(w, http.StatusBadRequest, "TProxy 端口为 0，请先配置端口")
			return
		}

		DisableTProxyRules() // 先清理，保证重复开启时幂等
		if err := EnableTProxyRules(port); err != nil {
			// 规则没装成功，状态必须回滚为关闭，避免「面板显示已启用、实际未生效」
			SetTproxyEnabled(false)
			log.Printf("[TProxy] 添加规则失败: %v", err)
			httpx.WriteJSONError(w, http.StatusInternalServerError, "启用 TProxy 失败: "+err.Error())
			return
		}
		SetTproxyEnabled(true)
		httpx.RespondJSON(w, http.StatusOK, map[string]bool{"enabled": true})

	default:
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
	}
}

// HandleTproxyExceptions 处理例外列表的获取和更新
func HandleTproxyExceptions(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		exceptionsMu.RLock()
		dst := tproxyDstExceptionsCache
		src := tproxySrcExceptionsCache
		exceptionsMu.RUnlock()
		httpx.RespondJSON(w, http.StatusOK, map[string]interface{}{
			"dst": dst,
			"src": src,
		})
	case http.MethodPost:
		var req struct {
			Dst []string `json:"dst"`
			Src []string `json:"src"`
		}
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			httpx.WriteJSONError(w, http.StatusBadRequest, "无效请求格式")
			return
		}
		// 分别保存
		if err := SaveTproxyDstExceptions(req.Dst); err != nil {
			httpx.WriteJSONError(w, http.StatusInternalServerError, "保存目的例外失败")
			return
		}
		if err := SaveTproxySrcExceptions(req.Src); err != nil {
			httpx.WriteJSONError(w, http.StatusInternalServerError, "保存源例外失败")
			return
		}
		// 如果 TProxy 启用则重载（经 GetTproxyState 读取，避免无锁访问）
		if GetTproxyState() {
			if port := tproxyPort(); port > 0 {
				DisableTProxyRules()
				if err := EnableTProxyRules(port); err != nil {
					log.Printf("[TProxy] 例外更新后重新应用规则失败: %v", err)
				}
			}
		}
		httpx.RespondJSON(w, http.StatusOK, map[string]string{"status": "ok"})
	default:
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
	}
}

// HandleTproxyProxyLocal 处理本机代理开关的获取和设置
func HandleTproxyProxyLocal(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		exceptionsMu.RLock()
		enabled := tproxyProxyLocal
		exceptionsMu.RUnlock()
		httpx.RespondJSON(w, http.StatusOK, map[string]bool{"enabled": enabled})
	case http.MethodPost:
		var req struct{ Enabled bool }
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			httpx.WriteJSONError(w, http.StatusBadRequest, "无效请求格式")
			return
		}
		if err := SaveTproxyProxyLocal(req.Enabled); err != nil {
			log.Printf("[TProxy] 保存本机代理开关失败: %v", err)
			httpx.WriteJSONError(w, http.StatusInternalServerError, "保存失败")
			return
		}
		// 如果 TProxy 当前启用，立即重新应用规则
		if GetTproxyState() {
			if port := tproxyPort(); port > 0 {
				DisableTProxyRules()
				if err := EnableTProxyRules(port); err != nil {
					log.Printf("[TProxy] 重新应用规则失败: %v", err)
				}
			}
		}
		httpx.RespondJSON(w, http.StatusOK, map[string]string{"status": "ok"})
	default:
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
	}
}

// HandleTproxyDNSRedirectPort 处理 DNS 重定向端口的读写。
//
// GET  返回 {port: int, default: 1053}
// PUT  body {port: int}，校验 1-65535 后持久化；若 TProxy 已启用则重建
//
//	规则让新端口立即生效，否则下次开关 TProxy 自然读取。
func HandleTproxyDNSRedirectPort(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		httpx.RespondJSON(w, http.StatusOK, map[string]int{
			"port":    GetDNSRedirectPort(),
			"default": DefaultDNSRedirectPort,
		})
	case http.MethodPut:
		var req struct {
			Port int `json:"port"`
		}
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			httpx.RespondJSON(w, http.StatusBadRequest, map[string]string{"error": "请求格式错误: " + err.Error()})
			return
		}
		if err := SetDNSRedirectPort(req.Port); err != nil {
			httpx.RespondJSON(w, http.StatusBadRequest, map[string]string{"error": err.Error()})
			return
		}
		// TProxy 已启用时重建规则让新端口立即生效
		if GetTproxyState() && tproxyPort() > 0 {
			DisableTProxyRules()
			if err := EnableTProxyRules(tproxyPort()); err != nil {
				log.Printf("[TProxy] DNS 端口变更后重建规则失败: %v", err)
			}
		}
		httpx.RespondJSON(w, http.StatusOK, map[string]int{"port": req.Port})
	default:
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
	}
}
