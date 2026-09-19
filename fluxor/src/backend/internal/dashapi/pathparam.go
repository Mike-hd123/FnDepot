package dashapi

import "strings"

// validateCorePathSuffix 校验从请求路径解析出的、将要拼进内核 API 路径的片段。
//
// 这些片段会被 core.CoreRequest 拼成 "http://localhost" + path 后转发给内核，
// 因此必须确保它们无法「穿越」到其它内核接口：
//   - 拒绝 "." 与 ".."    —— 路径穿越的基本单位
//   - 拒绝 "\" 与 NUL     —— 不同平台上可被视作分隔符
//   - 拒绝 %2e / %2f / %5c —— 处理函数多基于 EscapedPath()，这些编码形态
//     不会在解码层面被拦下，却会被内核或中间层还原成分隔符
//
// 允许片段内部含普通字符（含中文、空格、emoji），因为策略组/节点名可以包含它们。
func validateCorePathSuffix(p string) bool {
	if p == "" {
		return false
	}
	if strings.ContainsAny(p, "\\\x00") {
		return false
	}
	lower := strings.ToLower(p)
	if strings.Contains(lower, "%2e") || strings.Contains(lower, "%2f") || strings.Contains(lower, "%5c") {
		return false
	}
	for _, seg := range strings.Split(p, "/") {
		if seg == "." || seg == ".." {
			return false
		}
	}
	return true
}

// validateSinglePathSegment 校验必须是「单个」路径片段的情形（不含 "/"）。
func validateSinglePathSegment(seg string) bool {
	if seg == "" || strings.Contains(seg, "/") {
		return false
	}
	return validateCorePathSuffix(seg)
}
