package tproxy

import (
	"encoding/json"
	"fluxor/internal/config"
	"os"
	"path/filepath"
	"testing"
)

// withStartupStubs 把开机路径的规则安装/状态写入替换为桩，返回记录调用的闭包集。
// 测试结束后自动恢复真实实现。
type startupCalls struct {
	disableCount int
	enablePorts  []int
	states       []bool
	enableErr    error
}

func stubStartup(t *testing.T, c *startupCalls) {
	t.Helper()
	origEnable, origDisable, origSet := enableRulesFn, disableRulesFn, setStateFn
	t.Cleanup(func() {
		enableRulesFn, disableRulesFn, setStateFn = origEnable, origDisable, origSet
	})
	enableRulesFn = func(port int) error {
		c.enablePorts = append(c.enablePorts, port)
		return c.enableErr
	}
	disableRulesFn = func() { c.disableCount++ }
	// setStateFn 用真实持久化（写 temp 的 fluxor.json），只记录调用序列
	origPersist := setStateFn
	setStateFn = func(enabled bool) {
		c.states = append(c.states, enabled)
		origPersist(enabled)
	}
}

// useTempConfigFile 把 FluxorConfigFile 指向临时目录并返回路径。
func useTempConfigFile(t *testing.T) string {
	t.Helper()
	old := config.FluxorConfigFile
	dir := t.TempDir()
	config.FluxorConfigFile = filepath.Join(dir, "fluxor.json")
	t.Cleanup(func() { config.FluxorConfigFile = old })
	return config.FluxorConfigFile
}

// readTproxyKey 读磁盘上的 tproxy_enabled 键（值 + 存在性）。
func readTproxyKey(t *testing.T, path string) (val bool, exists bool) {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		return false, false
	}
	var full map[string]any
	if err := json.Unmarshal(data, &full); err != nil {
		t.Fatalf("fluxor.json 损坏: %v", err)
	}
	raw, ok := full["tproxy_enabled"]
	if !ok {
		return false, false
	}
	b, _ := raw.(bool)
	return b, true
}

func resetMemState() {
	tproxyMu.Lock()
	tproxyEnableState = false
	tproxyMu.Unlock()
}

// 首次安装（键从未写入）→ 默认开启、装规则、并持久化 true。
func TestResetOnStartupFirstInstallDefaultsOn(t *testing.T) {
	path := useTempConfigFile(t)
	calls := &startupCalls{}
	stubStartup(t, calls)
	resetMemState()

	ResetOnStartup(17898)

	if len(calls.enablePorts) != 1 || calls.enablePorts[0] != 17898 {
		t.Fatalf("期望按端口 17898 装一次规则, got %v", calls.enablePorts)
	}
	if !GetTproxyState() {
		t.Fatal("首次安装后内存状态应为开启")
	}
	if v, exists := readTproxyKey(t, path); !exists || !v {
		t.Fatalf("首次安装应持久化 tproxy_enabled=true, got val=%v exists=%v", v, exists)
	}
	// 清残留必在装规则之前
	if calls.disableCount < 1 {
		t.Fatal("应至少执行一次残留清理")
	}
}

// 重启且持久化为开 → 重装规则（nft 不跨重启）。
func TestResetOnStartupRestartEnabledReinstalls(t *testing.T) {
	path := useTempConfigFile(t)
	// 模拟上次会话已开：先真实写盘
	if err := persistTproxyEnabled(true); err != nil {
		t.Fatal(err)
	}
	calls := &startupCalls{}
	stubStartup(t, calls)
	resetMemState()

	ResetOnStartup(7898)

	if len(calls.enablePorts) != 1 || calls.enablePorts[0] != 7898 {
		t.Fatalf("持久化为开时应重装规则, got %v", calls.enablePorts)
	}
	if !GetTproxyState() {
		t.Fatal("状态应保持开启")
	}
	if v, _ := readTproxyKey(t, path); !v {
		t.Fatal("磁盘状态应保持 true")
	}
}

// 重启且持久化为关 → 不装规则，保持关闭。
func TestResetOnStartupRestartDisabledStaysOff(t *testing.T) {
	useTempConfigFile(t)
	if err := persistTproxyEnabled(false); err != nil {
		t.Fatal(err)
	}
	calls := &startupCalls{}
	stubStartup(t, calls)
	resetMemState()

	ResetOnStartup(7898)

	if len(calls.enablePorts) != 0 {
		t.Fatalf("持久化为关时不得装规则, got %v", calls.enablePorts)
	}
	if GetTproxyState() {
		t.Fatal("状态应为关闭")
	}
	if calls.disableCount < 1 {
		t.Fatal("即使为关也要清残留规则")
	}
}

// fail-safe：端口非法（<=0）→ 不装规则、状态回退为关（不出现「面板显示开、实际没生效」）。
func TestResetOnStartupInvalidPortFailsSafe(t *testing.T) {
	path := useTempConfigFile(t)
	if err := persistTproxyEnabled(true); err != nil {
		t.Fatal(err)
	}
	calls := &startupCalls{}
	stubStartup(t, calls)
	resetMemState()

	ResetOnStartup(0)

	if len(calls.enablePorts) != 0 {
		t.Fatalf("端口非法时不得装规则, got %v", calls.enablePorts)
	}
	if GetTproxyState() {
		t.Fatal("端口非法时状态必须回退为关")
	}
	if v, _ := readTproxyKey(t, path); v {
		t.Fatal("磁盘状态应同步回退为 false")
	}
}

// EnableTProxyRules 报错 → 状态回退为关。
func TestResetOnStartupEnableErrorRollsBack(t *testing.T) {
	path := useTempConfigFile(t)
	if err := persistTproxyEnabled(true); err != nil {
		t.Fatal(err)
	}
	calls := &startupCalls{enableErr: os.ErrPermission}
	stubStartup(t, calls)
	resetMemState()

	ResetOnStartup(7898)

	if GetTproxyState() {
		t.Fatal("装规则失败后状态应回退为关")
	}
	if v, _ := readTproxyKey(t, path); v {
		t.Fatal("装规则失败后磁盘状态应回退为 false")
	}
}
