package dashapi

import (
	"bytes"
	"encoding/json"
	"fluxor/internal/core"
	"fluxor/internal/httpx"
	"fluxor/internal/tproxy"
	"io"
	"log"
	"net/http"
	"strings"
)

// HandleConfigsAPI 处理配置的获取、修改和重载
func HandleConfigsAPI(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		resp, err := core.CoreRequest("GET", "/configs", nil)
		if err != nil {
			httpx.WriteJSONError(w, http.StatusBadGateway, "获取配置失败: "+err.Error())
			return
		}
		defer resp.Body.Close()
		w.Header().Set("Content-Type", "application/json")
		io.Copy(w, resp.Body)

	case http.MethodPatch:
		bodyBytes, err := io.ReadAll(r.Body)
		if err != nil {
			httpx.WriteJSONError(w, http.StatusBadRequest, "读取请求体失败: "+err.Error())
			return
		}
		r.Body = io.NopCloser(bytes.NewReader(bodyBytes))

		var fields map[string]interface{}
		if err := json.Unmarshal(bodyBytes, &fields); err == nil {
			if tpVal, ok := fields["tproxy-port"]; ok {
				if tpPort, ok := tpVal.(float64); ok {
					// 仅在 TProxy 处于启用状态时才重建规则。
					// 否则用户只要改一次端口，就会在开关为「关闭」的情况下
					// 被静默装上系统级透明代理规则（与 ReloadCore 的判定保持一致）。
					if tproxy.GetTproxyState() && tpPort > 0 {
						tproxy.DisableTProxyRules()
						if err := tproxy.EnableTProxyRules(int(tpPort)); err != nil {
							log.Printf("[TProxy] 端口变更后应用规则失败: %v", err)
						}
					} else {
						tproxy.DisableTProxyRules()
					}
				}
			}
		}

		resp, err := core.CoreRequest("PATCH", "/configs", r.Body)
		if err != nil {
			httpx.WriteJSONError(w, http.StatusBadGateway, "修改配置失败: "+err.Error())
			return
		}
		defer resp.Body.Close()
		w.WriteHeader(resp.StatusCode)
		io.Copy(w, resp.Body)

	case http.MethodPut:
		if err := core.ReloadCore(); err != nil {
			httpx.WriteJSONError(w, http.StatusInternalServerError, "重载配置失败: "+err.Error())
			return
		}
		w.WriteHeader(http.StatusNoContent)

	default:
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
	}
}

// HandleRestart 重启内核（POST /restart）
func HandleRestart(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		httpx.WriteJSONError(w, http.StatusMethodNotAllowed, "Method Not Allowed")
		return
	}
	resp, err := core.CoreRequest("POST", "/restart", strings.NewReader(`{"path": "", "payload": ""}`))
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "重启内核失败: "+err.Error())
		return
	}
	defer resp.Body.Close()
	w.WriteHeader(resp.StatusCode)
	io.Copy(w, resp.Body)
}

// HandleConfigsGeo 更新 GEO 数据库（POST /configs/geo）
func HandleConfigsGeo(w http.ResponseWriter, r *http.Request) {
	resp, err := core.CoreRequest("POST", "/configs/geo", nil)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "更新 GEO 失败: "+err.Error())
		return
	}
	defer resp.Body.Close()
	w.WriteHeader(resp.StatusCode)
}

// HandleProvidersGeo 更新 GEO 数据库（回退接口，POST /providers/geo）
func HandleProvidersGeo(w http.ResponseWriter, r *http.Request) {
	resp, err := core.CoreRequest("POST", "/providers/geo", nil)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "更新 GEO 失败: "+err.Error())
		return
	}
	defer resp.Body.Close()
	w.WriteHeader(resp.StatusCode)
}

// HandleFlushFakeIP 清空 FakeIP 缓存（POST /cache/fakeip/flush）
func HandleFlushFakeIP(w http.ResponseWriter, r *http.Request) {
	resp, err := core.CoreRequest("POST", "/cache/fakeip/flush", nil)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "清空 FakeIP 失败: "+err.Error())
		return
	}
	defer resp.Body.Close()
	w.WriteHeader(http.StatusOK)
}

// HandleFlushDNS 清空 DNS 缓存（POST /cache/dns/flush）
func HandleFlushDNS(w http.ResponseWriter, r *http.Request) {
	resp, err := core.CoreRequest("POST", "/cache/dns/flush", nil)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "清空 DNS 缓存失败: "+err.Error())
		return
	}
	defer resp.Body.Close()
	w.WriteHeader(http.StatusOK)
}

// HandleDNSQuery 执行 DNS 查询（代理 /dns/query）
func HandleDNSQuery(w http.ResponseWriter, r *http.Request) {
	name := r.URL.Query().Get("name")
	qtype := r.URL.Query().Get("type")
	path := "/dns/query?name=" + name + "&type=" + qtype
	resp, err := core.CoreRequest("GET", path, nil)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusBadGateway, "DNS 查询失败: "+err.Error())
		return
	}
	defer resp.Body.Close()
	w.Header().Set("Content-Type", "application/json")
	io.Copy(w, resp.Body)
}
