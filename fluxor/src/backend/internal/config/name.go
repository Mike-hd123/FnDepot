package config

import (
	"errors"
	"fmt"
	"strings"
	"unicode"
)

// MaxSubscriptionNameLength 订阅名长度上限（按字符计，非字节，以兼容 emoji）。
const MaxSubscriptionNameLength = 32

// 订阅名的清空后错误信息集中在此，便于前端与调用方复用同一套语义。
var (
	ErrSubscriptionNameEmpty   = errors.New("订阅名不能为空")
	ErrSubscriptionNameTooLong = fmt.Errorf("订阅名过长（最多 %d 个字符）", MaxSubscriptionNameLength)
	// ErrSubscriptionNameInvalidChar 的具体非法字符由 ValidateSubscriptionName 填写。
	ErrSubscriptionNameInvalidChar = errors.New("订阅名包含不允许的字符")
)

// ValidateSubscriptionName 校验订阅名。
//
// 允许的字符：中文、英文、数字、emoji。之所以严格限制，是因为订阅名会被用作：
//   - profiles/ 下的节点文件名（含 `../` 或 `/` 会造成路径穿越）
//   - 生成 config.yaml 中 proxy-providers 的键
//
// 因此不允许空格与任何标点（含 `-`、`_`、`.`、`/`、`:` 等）。
func ValidateSubscriptionName(name string) error {
	trimmed := strings.TrimSpace(name)
	if trimmed == "" {
		return ErrSubscriptionNameEmpty
	}
	// 首尾空白虽然会被 TrimSpace 处理，但调用方存的是原值，
	// 若不一致说明名称含前导/尾随空白，一并拒绝以免出现「看不见的差异」。
	if trimmed != name {
		return fmt.Errorf("%w: 名称首尾不能有空白", ErrSubscriptionNameInvalidChar)
	}

	runes := []rune(name)
	if len(runes) > MaxSubscriptionNameLength {
		return ErrSubscriptionNameTooLong
	}

	for _, r := range runes {
		if !isAllowedNameRune(r) {
			return fmt.Errorf("%w: %q", ErrSubscriptionNameInvalidChar, r)
		}
	}
	return nil
}

// isAllowedNameRune 判定单个字符是否属于「中文 / 英文 / 数字 / emoji」。
func isAllowedNameRune(r rune) bool {
	switch {
	case r >= 'a' && r <= 'z', r >= 'A' && r <= 'Z', r >= '0' && r <= '9':
		return true
	// 中日韩统一表意文字（含扩展 A 区），覆盖中文
	case unicode.Is(unicode.Han, r):
		return true
	// emoji 及各类符号表情
	case isEmoji(r):
		return true
	default:
		return false
	}
}

// isEmoji 判定字符是否属于 emoji 所在区段。
//
// 只覆盖 emoji 实际使用的码点区，避免把普通标点（如 °、§）误判为 emoji：
//   - U+1F300–U+1FAFF 各类图形与表情（含 🚀 等）
//   - U+1F1E6–U+1F1FF 区域指示符（国旗的一半，如 🇭🇰）
//   - U+2600–U+27BF   杂项符号与装饰符号（☀ ♻ ✂ 等）
//   - U+2190–U+21FF   箭头（emoji 变体）
//   - U+2B00–U+2BFF   杂项符号与箭头
//   - U+FE0F          变体选择符（emoji 呈现，常跟在 ☀ 之后）
//   - U+200D          零宽连接符（组合 emoji，如 👨‍👩‍👦）
func isEmoji(r rune) bool {
	switch {
	case r >= 0x1F300 && r <= 0x1FAFF,
		r >= 0x1F1E6 && r <= 0x1F1FF,
		r >= 0x2600 && r <= 0x27BF,
		r >= 0x2190 && r <= 0x21FF,
		r >= 0x2B00 && r <= 0x2BFF,
		r == 0xFE0F,
		r == 0x200D:
		return true
	default:
		return false
	}
}

// ValidateSubscriptionNames 校验一组订阅名，返回首个错误。
//
// 同时检查重名：订阅名对应独立的节点文件名与 provider 键，重名会互相覆盖。
func ValidateSubscriptionNames(subs []Subscription) error {
	seen := make(map[string]int, len(subs))
	for i, sub := range subs {
		if err := ValidateSubscriptionName(sub.Name); err != nil {
			return fmt.Errorf("第 %d 个订阅: %w", i+1, err)
		}
		if prev, dup := seen[sub.Name]; dup {
			return fmt.Errorf("第 %d 个订阅与第 %d 个重名: %s", i+1, prev+1, sub.Name)
		}
		seen[sub.Name] = i
	}
	return nil
}

// SanitizeSubscriptionFileName 由订阅名生成安全的节点文件名。
//
// 名称已经过 ValidateSubscriptionName 限制，此处再做一次防御：
// 剔除路径分隔符与上跳标记，确保结果始终是 proxies/ 下的单层文件名。
// 历史数据可能是在校验引入前写入的，故不能假设输入一定合法。
func SanitizeSubscriptionFileName(name string) string {
	trimmed := strings.TrimSpace(name)
	// 去掉任何路径成分（含 / 与 \）与上跳
	trimmed = strings.ReplaceAll(trimmed, "/", "")
	trimmed = strings.ReplaceAll(trimmed, "\\", "")
	trimmed = strings.ReplaceAll(trimmed, "..", "")
	if trimmed == "" {
		trimmed = "subscription"
	}
	return trimmed + ".yaml"
}
