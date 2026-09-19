package netinfo

import (
	"encoding/json"
	"fluxor/internal/core"
	"net/http"
)

// GetProxyPortFromConfig 从内核配置中获取代理端口
func GetProxyPortFromConfig() int {
	resp, err := core.CoreRequest("GET", "/configs", nil)
	if err != nil {
		return 0
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return 0
	}
	var cfg map[string]interface{}
	if err := json.NewDecoder(resp.Body).Decode(&cfg); err != nil {
		return 0
	}
	// 优先 mixed-port
	if p, ok := cfg["mixed-port"]; ok {
		if port, ok := p.(float64); ok && port > 0 {
			return int(port)
		}
	}
	if p, ok := cfg["port"]; ok {
		if port, ok := p.(float64); ok && port > 0 {
			return int(port)
		}
	}
	if p, ok := cfg["socks-port"]; ok {
		if port, ok := p.(float64); ok && port > 0 {
			return int(port)
		}
	}
	return 0
}
