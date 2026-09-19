package tproxy

import (
	"encoding/json"
	"fmt"
	"log"
	"os"

	"fluxor/internal/config"
)

// DefaultDNSRedirectPort 是 DNS 重定向的默认目标端口。
//
// 历史背景：fluxor 早期把 DNS 劫持目标硬编码为 1053（rules.go 里
// 4 处 `redirect to :1053`），而 1053 在部分环境里与上游/旁路组件
// （ClashLite 1054、系统内置 DNS）冲突，是 1.4.0-3 线上掉线报告的诱因之一。
// 现在把目标端口提升为持久化配置项 tproxy_dns_redirect_port，
// 默认值仍是 1053（保持向后兼容），用户可在面板按需改为 1054/53/其他。
const DefaultDNSRedirectPort = 1053

var tproxyDNSRedirectPort = DefaultDNSRedirectPort

// GetDNSRedirectPort 返回当前生效的 DNS 重定向端口。
func GetDNSRedirectPort() int {
	return tproxyDNSRedirectPort
}

// LoadTproxyDNSRedirectPort 从配置加载 DNS 重定向端口（启动时调用一次）。
// 缺字段/文件不存在/格式非法一律回落到默认值 1053，并打印日志说明，
// 绝不阻塞启动。
func LoadTproxyDNSRedirectPort() {
	data, err := os.ReadFile(config.FluxorConfigFile)
	if err != nil {
		tproxyDNSRedirectPort = DefaultDNSRedirectPort
		log.Printf("[TProxy] 读取配置失败，DNS 重定向端口回落到默认值 %d: %v", DefaultDNSRedirectPort, err)
		return
	}

	var cfg struct {
		TproxyDNSRedirectPort *int `json:"tproxy_dns_redirect_port"`
	}
	if err := json.Unmarshal(data, &cfg); err != nil || cfg.TproxyDNSRedirectPort == nil {
		tproxyDNSRedirectPort = DefaultDNSRedirectPort
		return
	}
	if *cfg.TproxyDNSRedirectPort < 1 || *cfg.TproxyDNSRedirectPort > 65535 {
		log.Printf("[TProxy] tproxy_dns_redirect_port=%d 越界，回落到默认值 %d",
			*cfg.TproxyDNSRedirectPort, DefaultDNSRedirectPort)
		tproxyDNSRedirectPort = DefaultDNSRedirectPort
		return
	}
	tproxyDNSRedirectPort = *cfg.TproxyDNSRedirectPort
	log.Printf("[TProxy] DNS 重定向端口已加载: %d", tproxyDNSRedirectPort)
}

// SetDNSRedirectPort 校验端口合法性后持久化到配置文件。
// 范围 1-65535；校验失败返回错误，不写文件、不改内存值。
func SetDNSRedirectPort(port int) error {
	if port < 1 || port > 65535 {
		return fmt.Errorf("DNS 重定向端口必须在 1-65535 之间，收到 %d", port)
	}

	var m map[string]interface{}
	data, err := os.ReadFile(config.FluxorConfigFile)
	if err != nil {
		// 首次设置时配置文件可能还不存在，此时从空 map 开始写，
		// 而不是拒绝——否则用户第一次改 DNS 端口会被「读不到文件」挡住。
		if !os.IsNotExist(err) {
			return fmt.Errorf("读取配置失败: %w", err)
		}
		m = make(map[string]interface{})
	} else if err := json.Unmarshal(data, &m); err != nil {
		return fmt.Errorf("解析配置失败: %w", err)
	} else if m == nil {
		m = make(map[string]interface{})
	}
	m["tproxy_dns_redirect_port"] = port

	out, err := json.MarshalIndent(m, "", "  ")
	if err != nil {
		return fmt.Errorf("序列化配置失败: %w", err)
	}
	if err := os.WriteFile(config.FluxorConfigFile, out, 0o644); err != nil {
		return fmt.Errorf("写入配置失败: %w", err)
	}

	tproxyDNSRedirectPort = port
	log.Printf("[TProxy] DNS 重定向端口已更新: %d", port)
	return nil
}
