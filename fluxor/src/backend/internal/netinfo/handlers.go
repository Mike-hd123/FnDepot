package netinfo

import (
	"encoding/json"
	"fluxor/internal/httpx"
	"fmt"
	"net/http"
)

// HandleLocalIPv4 获取本机 IPv4 及地理信息
func HandleLocalIPv4(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		httpx.WriteJSONError(w, http.StatusMethodNotAllowed, "Method Not Allowed")
		return
	}
	urls := []string{
		"https://ipv4.ddnspod.com/",
		"https://myip.ipip.net",
		"https://ip.3322.net",
	}
	ip, err := fetchPublicIPWithFallback(r.Context(), urls, "")
	if err != nil {
		// 回退到网卡获取的本机局域网 IP
		if localIP, localErr := getLocalIPFromInterfaces(false); localErr == nil {
			ip = localIP
		} else {
			httpx.WriteJSONError(w, http.StatusServiceUnavailable, "获取本机 IPv4 失败: "+err.Error())
			return
		}
	}
	// 获取地理信息
	country, region, isp := fetchGeoInfo(r.Context(), ip)
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{
		"ip":      ip,
		"country": country,
		"region":  region,
		"isp":     isp,
	})
}

// HandleLocalIPv6 获取本机 IPv6
func HandleLocalIPv6(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		httpx.WriteJSONError(w, http.StatusMethodNotAllowed, "Method Not Allowed")
		return
	}
	urls := []string{
		"https://ipv6.ddnspod.com",
		"https://v6.myip.la",
		"https://speed.neu6.edu.cn/getIP.php",
		"https://api6.ipify.org?format=json",
	}
	ip, err := fetchPublicIPWithFallback(r.Context(), urls, "")
	if err != nil {
		// 回退到网卡获取的本机全球单播 IPv6
		if localIP, localErr := getLocalIPFromInterfaces(true); localErr == nil {
			ip = localIP
		} else {
			httpx.WriteJSONError(w, http.StatusServiceUnavailable, "获取本机 IPv6 失败: "+err.Error())
			return
		}
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"ip": ip})
}

// HandleProxyIPv4 获取代理 IPv4 及地理信息
func HandleProxyIPv4(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		httpx.WriteJSONError(w, http.StatusMethodNotAllowed, "Method Not Allowed")
		return
	}
	proxyPort := GetProxyPortFromConfig()
	if proxyPort == 0 {
		httpx.WriteJSONError(w, http.StatusServiceUnavailable, "无可用代理端口")
		return
	}
	proxyAddr := fmt.Sprintf("http://127.0.0.1:%d", proxyPort)
	urls := []string{
		"https://api.ipify.org?format=json",
		"https://ipv4.icanhazip.com",
		"https://v4.ident.me",
	}
	ip, err := fetchPublicIPWithFallback(r.Context(), urls, proxyAddr)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusServiceUnavailable, "获取代理 IPv4 失败: "+err.Error())
		return
	}
	// 获取地理信息
	country, region, isp := fetchGeoInfo(r.Context(), ip)
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{
		"ip":      ip,
		"country": country,
		"region":  region,
		"isp":     isp,
	})
}

// HandleProxyIPv6 获取代理 IPv6
func HandleProxyIPv6(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		httpx.WriteJSONError(w, http.StatusMethodNotAllowed, "Method Not Allowed")
		return
	}
	proxyPort := GetProxyPortFromConfig()
	if proxyPort == 0 {
		httpx.WriteJSONError(w, http.StatusServiceUnavailable, "无可用代理端口")
		return
	}
	proxyAddr := fmt.Sprintf("http://127.0.0.1:%d", proxyPort)
	urls := []string{
		"https://api6.ipify.org?format=json",
		"https://ipv6.icanhazip.com",
		"https://v6.ident.me",
	}
	ip, err := fetchPublicIPWithFallback(r.Context(), urls, proxyAddr)
	if err != nil {
		httpx.WriteJSONError(w, http.StatusServiceUnavailable, "获取代理 IPv6 失败: "+err.Error())
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"ip": ip})
}
