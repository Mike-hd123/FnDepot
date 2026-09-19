package configgen

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"fluxor/internal/config"
)

// setWorkDir 把 config.CoreWorkDir 指向测试临时目录，返回还原函数。
func setWorkDir(t *testing.T) string {
	t.Helper()
	old := config.CoreWorkDir
	dir := t.TempDir()
	config.CoreWorkDir = dir
	t.Cleanup(func() { config.CoreWorkDir = old })
	return dir
}

func subs() []config.Subscription {
	return []config.Subscription{{Name: "my-sub"}}
}

// TestASNFallback_Disabled 无 ASN.mmdb 时 prefer-asn 降为 false。
func TestASNFallback_Disabled(t *testing.T) {
	setWorkDir(t) // 临时目录无 ASN.mmdb
	out := proxyGroupsFull(subs())
	if strings.Contains(out, "prefer-asn: true") {
		t.Fatalf("ASN.mmdb 缺失时 prefer-asn 应为 false，实际仍有 true：\n%s", out)
	}
	if !strings.Contains(out, "prefer-asn: false") {
		t.Fatalf("ASN.mmdb 缺失时 prefer-asn 应为 false：\n%s", out)
	}
}

// TestASNFallback_Enabled ASN.mmdb 存在时 prefer-asn 保持 true。
func TestASNFallback_Enabled(t *testing.T) {
	dir := setWorkDir(t)
	if err := os.WriteFile(filepath.Join(dir, "ASN.mmdb"), []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	out := proxyGroupsBase(subs())
	if !strings.Contains(out, "prefer-asn: true") {
		t.Fatalf("ASN.mmdb 存在时 prefer-asn 应为 true：\n%s", out)
	}
	// base 规则集也应含 5 个 smart 组（P0-3，上游已实现）
	if n := strings.Count(out, "type: smart"); n != 5 {
		t.Errorf("base 规则集应有 5 个 smart 组，实际 %d 个", n)
	}
}
