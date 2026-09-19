package configgen

import (
	"fluxor/internal/config"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestSmartGroupsSurviveRegeneration 核心验收：
// 订阅更新触发 config.yaml 全量重写后，smart 组定义必须存活。
func TestSmartGroupsSurviveRegeneration(t *testing.T) {
	dir := t.TempDir()
	target := filepath.Join(dir, "config.yaml")
	oldTarget := config.ConfigTarget
	oldSocket := config.CoreSocket
	defer func() { config.ConfigTarget, config.CoreSocket = oldTarget, oldSocket }()
	config.ConfigTarget = target
	config.CoreSocket = filepath.Join(dir, "fluxor.sock")

	cfg := config.SubscribeConfig{
		ProxyPort:   17890,
		TproxyPort:  17898,
		PanelPort:   19090,
		RuleGroup:   "full",
		UIPanel:     "metacubexd",
		Subscriptions: []config.Subscription{
			{Name: "myairport", URL: "https://example.com/sub1", UpdateInterval: 86400, HealthInterval: 300},
		},
	}

	if err := GenerateConfig(cfg); err != nil {
		t.Fatalf("GenerateConfig: %v", err)
	}
	b, err := os.ReadFile(target)
	if err != nil {
		t.Fatalf("read: %v", err)
	}
	s := string(b)

	// 第一次生成：五... [trimmed for display]
	for _, g := range []string{"🇭🇰 香港节点", "🇹🇼 台湾节点", "🇯🇵 日本节点", "🇸🇬 新加坡节点", "🇺🇸 美国节点"} {
		// 上游 yaml.v3 节点渲染后，flow 序列化会对含中文/空格的组名加双引号
		// （name: "🇭🇰 香港节点"），兼容带引号与不带引号两种形态。
		if !strings.Contains(s, `name: "`+g+`"`) && !strings.Contains(s, "name: "+g) {
			t.Fatalf("missing group %s", g)
		}
	}
	if n := strings.Count(s, "type: smart"); n != 5 {
		t.Fatalf("expect 5 smart groups, got %d\n%s", n, s)
	}
	for _, kw := range []string{"uselightgbm: true", "collectdata: true", "include-all: true", "smart-collector-size: 100"} {
		if !strings.Contains(s, kw) {
			t.Fatalf("missing %q in generated config", kw)
		}
	}
	if !strings.Contains(s, "use: [myairport]") {
		t.Fatal("subscription provider name not injected into smart groups")
	}

	// 模拟订阅更新再次全量重写
	cfg.Subscriptions[0].UpdateInterval = 43200
	if err := GenerateConfig(cfg); err != nil {
		t.Fatalf("regenerate: %v", err)
	}
	b2, _ := os.ReadFile(target)
	s2 := string(b2)
	if n := strings.Count(s2, "type: smart"); n != 5 {
		t.Fatalf("after regeneration smart groups lost: %d\n%s", n, s2)
	}
	if !strings.Contains(s2, "uselightgbm: true") {
		t.Fatal("uselightgbm lost after regeneration")
	}
	t.Log("smart groups survive full rewrite ✓")
}
