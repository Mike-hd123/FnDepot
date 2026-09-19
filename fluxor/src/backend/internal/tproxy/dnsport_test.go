package tproxy

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"fluxor/internal/config"
)

// setFluxorConfig 把 config.FluxorConfigFile 指向测试临时目录，返回还原函数。
func setFluxorConfig(t *testing.T) string {
	t.Helper()
	old := config.FluxorConfigFile
	dir := t.TempDir()
	config.FluxorConfigFile = filepath.Join(dir, "fluxor.conf.json")
	t.Cleanup(func() { config.FluxorConfigFile = old })
	return dir
}

// 每个测试先重置全局内存值，避免测试间串扰（其它测试会改写 tproxyDNSRedirectPort）。
func resetDNSPort() {
	exceptionsMu.Lock()
	tproxyDNSRedirectPort = DefaultDNSRedirectPort
	exceptionsMu.Unlock()
}

// TestDNSRedirectPort_DefaultWhenFileMissing 配置不存在时回落默认 1053。
func TestDNSRedirectPort_DefaultWhenFileMissing(t *testing.T) {
	setFluxorConfig(t) // 不创建配置文件
	resetDNSPort()
	LoadTproxyDNSRedirectPort()
	if got := GetDNSRedirectPort(); got != 1053 {
		t.Fatalf("配置缺失时应回落默认 1053，实际 %d", got)
	}
}

// TestDNSRedirectPort_RoundTrip 写配置后读回同一值。
func TestDNSRedirectPort_RoundTrip(t *testing.T) {
	dir := setFluxorConfig(t)
	resetDNSPort()

	// 初始为默认值
	if err := SetDNSRedirectPort(1054); err != nil {
		t.Fatalf("设置 1054 应成功: %v", err)
	}
	if got := GetDNSRedirectPort(); got != 1054 {
		t.Fatalf("内存值应为 1054，实际 %d", got)
	}

	// 重新加载（模拟重启）应读回 1054
	tproxyDNSRedirectPort = DefaultDNSRedirectPort
	LoadTproxyDNSRedirectPort()
	if got := GetDNSRedirectPort(); got != 1054 {
		t.Fatalf("重启后应读回 1054，实际 %d", got)
	}

	// 边界值
	if err := SetDNSRedirectPort(65535); err != nil {
		t.Fatalf("设置 65535 应成功: %v", err)
	}
	if err := SetDNSRedirectPort(1); err != nil {
		t.Fatalf("设置 1 应成功: %v", err)
	}

	// 文件内容确实落盘
	data, err := os.ReadFile(config.FluxorConfigFile)
	if err != nil {
		t.Fatalf("配置文件应存在: %v", err)
	}
	if !strings.Contains(string(data), "tproxy_dns_redirect_port") {
		t.Fatalf("配置文件应含 tproxy_dns_redirect_port 字段")
	}
	_ = dir
}

// TestDNSRedirectPort_OutOfRangeRejected 越界值被拒绝，不写文件、不改内存。
func TestDNSRedirectPort_OutOfRangeRejected(t *testing.T) {
	setFluxorConfig(t)
	resetDNSPort()
	for _, bad := range []int{0, -1, 65536, 100000, -1000} {
		if err := SetDNSRedirectPort(bad); err == nil {
			t.Errorf("端口 %d 应被拒绝", bad)
		}
	}
	// 内存值未被污染
	if got := GetDNSRedirectPort(); got != 1053 {
		t.Fatalf("越界写入后内存值应仍为 1053，实际 %d", got)
	}
	// 文件未被创建（SetDNSRedirectPort 在读取失败时返回错误，不写文件）
	if _, err := os.Stat(config.FluxorConfigFile); !os.IsNotExist(err) {
		t.Fatalf("越界写入不应创建配置文件")
	}
}

// TestIsTUNEnabled 后端兜底：TUN 开启时 EnableTProxyRules 必须拒绝。
func TestIsTUNEnabled(t *testing.T) {
	dir := setFluxorConfig(t)
	_ = dir

	// 无 tun 字段 → false
	if IsTUNEnabled() {
		t.Fatalf("无 tun 字段时应视为 TUN 未启用")
	}

	// tun.enable=true → true
	cfg := `{"tun":{"enable":true}}`
	if err := os.WriteFile(config.FluxorConfigFile, []byte(cfg), 0o644); err != nil {
		t.Fatal(err)
	}
	if !IsTUNEnabled() {
		t.Fatalf("tun.enable=true 时应为 true")
	}

	// TUN 开启时 EnableTProxyRules 必须返回错误（后端兜底）
	if err := EnableTProxyRules(1053); err == nil {
		t.Fatalf("TUN 开启时 EnableTProxyRules 应拒绝，实际返回 nil")
	}

	// tun.enable=false → false，EnableTProxyRules 不再因互斥拒绝
	cfg = `{"tun":{"enable":false}}`
	os.WriteFile(config.FluxorConfigFile, []byte(cfg), 0o644)
	if IsTUNEnabled() {
		t.Fatalf("tun.enable=false 时应为 false")
	}
}
