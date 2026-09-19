package buildinfo

import "strings"

// DefaultVersion 未显式注入版本号时使用的版本。
//
// make 与 go build 不带版本号时都会编译出该版本；这样本地自行编译的产物
// 也是一个「可比较的真实版本」，更新检查能正常判断是否有新版本。
const DefaultVersion = "1.0.0"

// Version 由 -ldflags -X 在编译期注入，例如：
//
//	go build -ldflags "-X fluxor/internal/buildinfo.Version=2.3.4"
//	make V=2.3.4
//
// 未注入时即为 DefaultVersion（1.0.0）。
var Version = DefaultVersion

// Name 返回用于展示与更新的版本名。
//
// 去掉可能存在的 v/V 前缀，使 "v1.0.0" 与 "1.0.0" 在比较与展示时表现一致。
func Name() string {
	return strings.TrimPrefix(strings.TrimPrefix(Version, "v"), "V")
}

// IsKnown 报告版本号是否可用于比较。
//
// 正常构建（含未注入时的默认值 1.0.0）都返回 true。仅当版本被显式设为空
// 或占位值 "dev" 时返回 false——后者是留给「本地调试、不希望参与更新判断」
// 的逃生口（make V=dev）。此时调用方应跳过更新判断，而不是假定有新版本。
func IsKnown() bool {
	n := Name()
	return n != "" && n != "dev"
}
