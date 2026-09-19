package config

// SubscribeConfig 订阅配置结构体
type SubscribeConfig struct {
	ProxyPort          int            `json:"proxy_port"`
	TproxyPort         int            `json:"tproxy_port"`
	PanelPort          int            `json:"panel_port"`
	PanelSecret        string         `json:"panel_secret"`
	RuleGroup          string         `json:"rule_group"`
	UIPanel            string         `json:"ui_panel"`
	MetaBackendURL     string         `json:"meta_backend_url"`
	Mode               string         `json:"mode"`
	ActiveSubscription string         `json:"active_subscription"`
	Subscriptions      []Subscription `json:"subscriptions"`
	DeletePhysical     []string       `json:"delete_physical,omitempty"`
}

// Subscription 描述单个订阅源及其最近一次的更新元数据。
type Subscription struct {
	Name             string                 `json:"name"`
	URL              string                 `json:"url"`
	UpdateInterval   int                    `json:"update_interval"`
	HealthInterval   int                    `json:"health_interval"`
	Prefix           string                 `json:"prefix"`
	UpdatedAt        string                 `json:"updated_at,omitempty"`
	SubscriptionInfo map[string]interface{} `json:"subscription_info,omitempty"`
}
