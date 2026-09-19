package netinfo

import (
	"encoding/json"
	"fluxor/internal/httpx"
	"fmt"
	"net"
	"net/http"
)

// HandleInterfaces 返回系统所有的物理网络接口名称（GET /interfaces）
func HandleInterfaces(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		return
	}

	ifaces, err := net.Interfaces()
	if err != nil {
		httpx.WriteJSONError(w, http.StatusInternalServerError, "获取网络接口失败: "+err.Error())
		return
	}

	var names []string
	for _, iface := range ifaces {
		// 过滤回环接口和未启用的接口
		if (iface.Flags & net.FlagLoopback) != 0 {
			continue
		}
		if (iface.Flags & net.FlagUp) == 0 {
			continue
		}
		names = append(names, iface.Name)
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(names)
}

// getLocalIPFromInterfaces 从本地网卡获取有效的 IPv4 或全球单播 IPv6 地址作为回退展示
func getLocalIPFromInterfaces(isIPv6 bool) (string, error) {
	ifaces, err := net.Interfaces()
	if err != nil {
		return "", err
	}
	for _, iface := range ifaces {
		if (iface.Flags&net.FlagUp) == 0 || (iface.Flags&net.FlagLoopback) != 0 {
			continue
		}
		addrs, err := iface.Addrs()
		if err != nil {
			continue
		}
		for _, addr := range addrs {
			var ip net.IP
			switch v := addr.(type) {
			case *net.IPNet:
				ip = v.IP
			case *net.IPAddr:
				ip = v.IP
			}
			if ip == nil || ip.IsLoopback() {
				continue
			}
			if isIPv6 {
				// 获取非链路本地且合法的全球单播 IPv6 地址 (通常为公网分发 IPv6)
				if ip.To4() == nil && ip.IsGlobalUnicast() && !ip.IsLinkLocalUnicast() {
					return ip.String(), nil
				}
			} else {
				// 获取首个有效的局域网/公网 IPv4
				if ip.To4() != nil {
					return ip.String(), nil
				}
			}
		}
	}
	return "", fmt.Errorf("no active interface IP found")
}
