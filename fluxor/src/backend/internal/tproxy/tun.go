package tproxy

import (
	"encoding/json"
	"log"
	"os"

	"fluxor/internal/config"
)

// IsTUNEnabled 读取 Fluxor 持久化配置里的 tun.enable 字段，用于后端对
// TProxy 与 TUN 互斥规则的兜底校验。
//
// 为什么后端也要查：前端 Config.vue 的 saveTun/toggleTProxy 已做了互斥
// （开 TUN 就关 TProxy，开 TProxy 就关 TUN），但那只拦 UI 路径。以下
// 场景绕过前端直连后端，会让 TUN 和 TProxy 同时处于启用状态——两套
// 都做出口流量重定向，叠加后本机流量进入环回，表现为整站打不开：
//
//   - cron job / 脚本直接调后端启停接口
//   - 用户手改 fluxor.conf.json 的 tun.enable 后重启面板
//   - 第三方客户端按 OpenAPI 文档调用（互斥只在 Vue 里，不在契约里）
//
// 因此后端在真正落 nft 规则前再做一次校验，互斥兜底落在离规则最近的地方。
//
// 读不到配置文件 / 缺字段一律返回 false（视为 TUN 未启用，不阻断 TProxy）——
// 保守方向：宁可放行 TProxy，也不能因为配置文件暂时不可读就把用户
// 刚点下的开关悄悄关掉。
func IsTUNEnabled() bool {
	data, err := os.ReadFile(config.FluxorConfigFile)
	if err != nil {
		return false
	}
	var cfg struct {
		Tun struct {
			Enable bool `json:"enable"`
		} `json:"tun"`
	}
	if err := json.Unmarshal(data, &cfg); err != nil {
		log.Printf("[TProxy] 读取 tun.enable 失败，视为未启用: %v", err)
		return false
	}
	return cfg.Tun.Enable
}
