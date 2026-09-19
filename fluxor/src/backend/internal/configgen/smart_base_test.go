package configgen

import (
	"fluxor/internal/config"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestSmartGroupsInBase 对称于 TestSmartGroupsSurviveRegeneration：
// 1.4.0-1 起 base 规则集（前端默认）也要生成 5 个 smart 地区组，
// 否则用户默认装完看不到 smart 优化（09-18 用户反馈根因）。
func TestSmartGroupsInBase(t *testing.T) {
	dir := t.TempDir()
	target := filepath.Join(dir, "config.yaml")
	oldTarget := config.ConfigTarget
	oldSocket := config.CoreSocket
	defer func() { config.ConfigTarget, config.CoreSocket = oldTarget, oldSocket }()
	config.ConfigTarget = target
	config.CoreSocket = filepath.Join(dir, "fluxor.sock")

	cfg := config.SubscribeConfig{
		ProxyPort:  17890,
		TproxyPort: 17898,
		PanelPort:  19090,
		RuleGroup:  "base",
		UIPanel:    "metacubexd",
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

	// 与 TestSmartGroupsSurviveRegeneration 同款：兼容带引号与不带引号两种形态
	for _, g := range []string{"🇭🇰 香港节点", "🇹🇼 台湾节点", "🇯🇵 日本节点", "🇸🇬 新加坡节点", "🇺🇸 美国节点"} {
		if !strings.Contains(s, `name: "`+g+`"`) && !strings.Contains(s, "name: "+g) {
			t.Fatalf("missing smart group %q in base", g)
		}
	}
	if n := strings.Count(s, "type: smart"); n != 5 {
		t.Fatalf("expect 5 smart groups in base, got %d\n%s", n, s)
	}
	for _, kw := range []string{"uselightgbm: true", "collectdata: true", "include-all: true", "smart-collector-size: 100"} {
		if !strings.Contains(s, kw) {
			t.Fatalf("missing %q in base config", kw)
		}
	}
	if !strings.Contains(s, "use: [myairport]") {
		t.Fatal("subscription provider name not injected into base smart groups")
	}
	t.Log("smart groups present in base rule group ✓")
}
