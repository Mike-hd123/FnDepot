package config

import "testing"

// TestWithDefaultsZeroStruct 完全空的结构体应被补成出厂默认值。
//
// 这正是 1.4.0-1 的故障场景：POST /subscribe/generate -d '{}' 解出的 SubscribeConfig
// 全是零值，若不兜底就会生成端口全 0、无规则集的空壳 config.yaml。
func TestWithDefaultsZeroStruct(t *testing.T) {
	c := SubscribeConfig{}.WithDefaults()

	if c.ProxyPort != 7890 {
		t.Errorf("ProxyPort = %d, want 7890", c.ProxyPort)
	}
	if c.PanelPort != 9090 {
		t.Errorf("PanelPort = %d, want 9090", c.PanelPort)
	}
	if c.TproxyPort != 7898 {
		t.Errorf("TproxyPort = %d, want 7898", c.TproxyPort)
	}
	if c.RuleGroup != "base" {
		t.Errorf("RuleGroup = %q, want \"base\"", c.RuleGroup)
	}
	if c.UIPanel != "metacubexd" {
		t.Errorf("UIPanel = %q, want \"metacubexd\"", c.UIPanel)
	}
	if c.Mode != "merge" {
		t.Errorf("Mode = %q, want \"merge\"", c.Mode)
	}
	// nil 列表要变成非 nil 空切片，避免下游 len()==0 分支走错
	if c.Subscriptions == nil {
		t.Error("Subscriptions should be non-nil empty slice")
	}
	if len(c.Subscriptions) != 0 {
		t.Errorf("Subscriptions = %v, want empty", c.Subscriptions)
	}
}

// TestWithDefaultsPreservesExplicit 已设置的值必须原样保留，包括用户有意改的端口。
func TestWithDefaultsPreservesExplicit(t *testing.T) {
	c := SubscribeConfig{
		ProxyPort:          17890,
		PanelPort:          19090,
		TproxyPort:         17898,
		RuleGroup:          "full",
		UIPanel:            "metacubexd",
		MetaBackendURL:     "http://10.0.0.2:9090",
		Mode:               "switch",
		ActiveSubscription: "yfq",
		Subscriptions:      []Subscription{{Name: "yfq", URL: "http://example.com/s"}},
	}.WithDefaults()

	if c.ProxyPort != 17890 || c.PanelPort != 19090 || c.TproxyPort != 17898 {
		t.Errorf("ports overwritten: %+v", c)
	}
	if c.RuleGroup != "full" || c.Mode != "switch" || c.ActiveSubscription != "yfq" {
		t.Errorf("strings overwritten: %+v", c)
	}
	if c.MetaBackendURL != "http://10.0.0.2:9090" {
		t.Errorf("MetaBackendURL overwritten: %q", c.WithDefaults().MetaBackendURL)
	}
	if len(c.Subscriptions) != 1 || c.Subscriptions[0].Name != "yfq" {
		t.Errorf("Subscriptions overwritten: %+v", c.Subscriptions)
	}
}

// TestWithDefaultsPartialMerge 只补零值字段，不动已设字段。
//
// 对应 generate 里的「磁盘已清空、内存只剩一半」这类半残状态。
func TestWithDefaultsPartialMerge(t *testing.T) {
	c := SubscribeConfig{
		ProxyPort: 17890,
		RuleGroup: "smart",
		Subscriptions: []Subscription{
			{Name: "yfq", URL: "http://example.com/s"},
		},
	}.WithDefaults()

	if c.ProxyPort != 17890 {
		t.Errorf("ProxyPort = %d, want 17890", c.ProxyPort)
	}
	if c.PanelPort != 9090 {
		t.Errorf("PanelPort = %d, want 9090", c.PanelPort)
	}
	if c.TproxyPort != 7898 {
		t.Errorf("TproxyPort = %d, want 7898", c.TproxyPort)
	}
	if c.RuleGroup != "smart" {
		t.Errorf("RuleGroup = %q, want \"smart\"", c.RuleGroup)
	}
	if c.Mode != "merge" {
		t.Errorf("Mode = %q, want \"merge\"", c.Mode)
	}
	if len(c.Subscriptions) != 1 {
		t.Errorf("Subscriptions = %v, want 1 entry", c.Subscriptions)
	}
}

// TestWithDefaultsIsPure WithDefaults 不得修改接收者（值接收者语义）。
func TestWithDefaultsIsPure(t *testing.T) {
	orig := SubscribeConfig{}
	_ = orig.WithDefaults()
	if orig.ProxyPort != 0 || orig.RuleGroup != "" || orig.Subscriptions != nil {
		t.Errorf("receiver mutated: %+v", orig)
	}
}
