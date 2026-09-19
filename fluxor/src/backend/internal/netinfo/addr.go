package netinfo

import (
	"fmt"
	"net"
	"strconv"
)

// ValidateTCPAddr 校验 TCP 地址格式（IP:Port 或 :Port）
func ValidateTCPAddr(addr string) error {
	if addr == "" {
		return nil
	}
	host, port, err := net.SplitHostPort(addr)
	if err != nil {
		return fmt.Errorf("地址格式错误: %w", err)
	}
	if host != "" {
		if ip := net.ParseIP(host); ip == nil {
			return fmt.Errorf("无效的 IP 地址: %s", host)
		}
	}
	p, err := strconv.Atoi(port)
	if err != nil || p < 1 || p > 65535 {
		return fmt.Errorf("端口无效: %s", port)
	}
	return nil
}
