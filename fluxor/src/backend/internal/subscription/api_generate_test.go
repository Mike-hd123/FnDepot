package subscription

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"

	"fluxor/internal/config"
)

// setConfig 把 cfg 装进内存态（当前生效配置），并在测试结束后还原。
func setConfig(t *testing.T, cfg config.SubscribeConfig) {
	t.Helper()
	old := config.Current
	config.Mu.Lock()
	config.Current = cfg
	config.Mu.Unlock()
	t.Cleanup(func() {
		config.Mu.Lock()
		config.Current = old
		config.Mu.Unlock()
	})
}

// TestMergeEmptyRequestKeepsCurrent 空请求体（{}）必须完整保留内存态。
//
// 这是 1.4.0-1 的现网故障：运维 POST /subscribe/generate -d '{}' 让内核重跑，
// 结果订阅列表、端口、规则集被全量清空并写盘，config.yaml 退化成 8 行端口全 0
// 的空壳，热重载后没有任何代理可用。
func TestMergeEmptyRequestKeepsCurrent(t *testing.T) {
	setConfig(t, config.SubscribeConfig{
		ProxyPort:          17890,
		PanelPort:          19090,
		TproxyPort:         17898,
		RuleGroup:          "base",
		UIPanel:            "metacubexd",
		Mode:               "merge",
		ActiveSubscription: "yfq",
		Subscriptions: []config.Subscription{
			{Name: "yfq", URL: "http://192.168.5.2:1213/yfq", UpdateInterval: 86400, HealthInterval: 300},
		},
	})

	got := mergeSubscribeConfig(config.SubscribeConfig{})

	if got.ProxyPort != 17890 || got.PanelPort != 19090 || got.TproxyPort != 17898 {
		t.Errorf("ports lost on empty request: %+v", got)
	}
	if got.RuleGroup != "base" || got.UIPanel != "metacubexd" || got.Mode != "merge" {
		t.Errorf("strings lost on empty request: %+v", got)
	}
	if got.ActiveSubscription != "yfq" {
		t.Errorf("active_subscription lost on empty request: %q", got.ActiveSubscription)
	}
	if len(got.Subscriptions) != 1 || got.Subscriptions[0].Name != "yfq" {
		t.Errorf("subscriptions lost on empty request: %+v", got.Subscriptions)
	}
}

// TestMergePartialRequestOverridesOnlySubmitted 部分提交只覆盖提交的字段。
func TestMergePartialRequestOverridesOnlySubmitted(t *testing.T) {
	setConfig(t, config.SubscribeConfig{
		ProxyPort:  17890,
		PanelPort:  19090,
		RuleGroup:  "base",
		UIPanel:    "metacubexd",
		Mode:       "merge",
		ActiveSubscription: "yfq",
		Subscriptions: []config.Subscription{
			{Name: "yfq", URL: "http://x/y"},
		},
	})

	got := mergeSubscribeConfig(config.SubscribeConfig{
		RuleGroup: "smart",
		Subscriptions: []config.Subscription{
			{Name: "yfq", URL: "http://x/y2"},
			{Name: "other", URL: "http://x/z"},
		},
	})

	if got.RuleGroup != "smart" {
		t.Errorf("submitted RuleGroup not applied: %q", got.RuleGroup)
	}
	if got.ProxyPort != 17890 || got.PanelPort != 19090 {
		t.Errorf("untouched ports changed: %+v", got)
	}
	if got.Mode != "merge" || got.UIPanel != "metacubexd" {
		t.Errorf("untouched strings changed: %+v", got)
	}
	// 列表型字段以请求体为准：加订阅能加进来
	if len(got.Subscriptions) != 2 {
		t.Fatalf("subscriptions not replaced: %+v", got.Subscriptions)
	}
	if got.Subscriptions[0].URL != "http://x/y2" || got.Subscriptions[1].Name != "other" {
		t.Errorf("subscriptions not taken from request: %+v", got.Subscriptions)
	}
	// 空列表代表「用户确实删光了」，不能被旧值顶回
	got2 := mergeSubscribeConfig(config.SubscribeConfig{Subscriptions: []config.Subscription{}})
	if len(got2.Subscriptions) != 0 {
		t.Errorf("explicit empty list should stay empty, got %+v", got2.Subscriptions)
	}
}

// TestGenerateEmptyBodyDoesNotWipeConfig 端到端：POST {} 不会清空持久化配置。
//
// 这是 1.4.0-1 故障的直接回归测试。generate 会触发下载/重载内核，所以只验证
// 到「内存与磁盘配置未被清空」这一层，用 httptest 断言响应后 config.Current 与
// 磁盘 fluxor.json 都还保留原订阅。
func TestGenerateEmptyBodyDoesNotWipeConfig(t *testing.T) {
	dir := t.TempDir()
	origTarget, origFile, origWorkDir, origCoreSocket :=
		config.ConfigTarget, config.FluxorConfigFile, config.CoreWorkDir, config.CoreSocket
	t.Cleanup(func() {
		config.ConfigTarget, config.FluxorConfigFile, config.CoreWorkDir, config.CoreSocket =
			origTarget, origFile, origWorkDir, origCoreSocket
	})
	config.FluxorConfigFile = dir + "/fluxor.json"
	config.CoreWorkDir = dir
	config.ConfigTarget = dir + "/config.yaml"
	config.CoreSocket = dir + "/fluxor.sock"

	// 先持久化一份真实配置，模拟装完订阅后的磁盘状态
	setConfig(t, config.SubscribeConfig{
		ProxyPort:          17890,
		PanelPort:          19090,
		TproxyPort:         17898,
		RuleGroup:          "base",
		UIPanel:            "metacubexd",
		Mode:               "merge",
		ActiveSubscription: "yfq",
		Subscriptions: []config.Subscription{
			{Name: "yfq", URL: "http://x/yfq", UpdateInterval: 86400, HealthInterval: 300},
		},
	})
	if err := config.SaveSubscribeConfig(); err != nil {
		t.Fatalf("seed config: %v", err)
	}

	// 用空 body 调 generate，模拟 curl -d '{}'。
	// 内核不在位会返回 warning 而不是 500，关键断言是配置没被清空。
	req := httptest.NewRequest(http.MethodPost, "/subscribe/generate",
		strings.NewReader("{}"))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	HandleGenerateConfig(rec, req)

	if rec.Code < 200 || rec.Code >= 300 {
		t.Fatalf("status = %d, body = %s", rec.Code, rec.Body.String())
	}

	// 磁盘不能被写成空壳
	cfg, err := loadConfigFromFile(config.FluxorConfigFile)
	if err != nil {
		t.Fatalf("reload from disk: %v", err)
	}
	if len(cfg.Subscriptions) != 1 || cfg.Subscriptions[0].Name != "yfq" {
		t.Errorf("disk config wiped by empty generate: %+v", cfg)
	}
	if cfg.ProxyPort != 17890 || cfg.PanelPort != 19090 {
		t.Errorf("disk ports zeroed by empty generate: %+v", cfg)
	}
	if cfg.RuleGroup != "base" {
		t.Errorf("disk rule_group emptied by empty generate: %q", cfg.RuleGroup)
	}

	// 内存态同样不能空
	config.Mu.RLock()
	cur := config.Current
	config.Mu.RUnlock()
	if len(cur.Subscriptions) != 1 || cur.ActiveSubscription != "yfq" {
		t.Errorf("memory config wiped: %+v", cur)
	}
}

// loadConfigFromFile 直接从磁盘读一份配置，供断言用（不走全局 Current）。
func loadConfigFromFile(path string) (config.SubscribeConfig, error) {
	var cfg config.SubscribeConfig
	b, err := os.ReadFile(path)
	if err != nil {
		return cfg, err
	}
	return cfg, json.Unmarshal(b, &cfg)
}
