package subscription

import "fluxor/internal/config"

// activeSubscriptionExists 判定 cfg.ActiveSubscription 是否为当前订阅列表中的真实成员。
//
// active_subscription 以「订阅名」为键存储，而订阅名可被改名或整体删除；
// 客户端在改名/删除后仍可能提交旧名（历史数据里也可能残留）。
// 此时若只判空字符串，后续流程会按旧名拼出 proxies/<旧名>.yaml 并复制该文件，
// 造成「前端显示新订阅名、内核实际生效旧订阅配置」的静默错配。
func activeSubscriptionExists(cfg config.SubscribeConfig) bool {
	if cfg.ActiveSubscription == "" {
		return false
	}
	for _, s := range cfg.Subscriptions {
		if s.Name == cfg.ActiveSubscription {
			return true
		}
	}
	return false
}
