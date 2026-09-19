package configcheck

import (
	"errors"
	"fmt"
	"strings"

	"gopkg.in/yaml.v3"
)

// 主配置必须具备的顶层字段之一。任一存在即视为可被内核加载的节点配置。
//
// 与内核行为保持一致：mihomo 解析顶层键时大小写不敏感（实测 `Proxies:`、
// `PROXIES:` 均可正常加载），因此这里同样按小写归一后比较。
var requiredTopLevelKeys = []string{"proxies", "proxy-providers", "proxy-groups"}

// 允许的顶层字段类型：proxies/proxy-groups 是序列，proxy-providers 是映射。
// 逐个声明是为了给出准确的错误信息，而不是笼统的「类型不对」。
var keyExpectedKind = map[string]string{
	"proxies":         "列表",
	"proxy-groups":    "列表",
	"proxy-providers": "映射",
}

// ValidateClashConfig 字段级校验内容是否为可被内核加载的 Clash 配置。
//
// 校验规则：
//  1. 必须能解析为 YAML 映射（排除 Base64 blob、URI 列表、HTML/JSON 错误页等）
//  2. 必须至少含 proxies / proxy-providers / proxy-groups 之一（键名大小写不敏感）
//  3. 上述字段的类型必须与内核预期一致
//
// 返回的 error 已包含可直接展示给用户的原因。
func ValidateClashConfig(content []byte) error {
	trimmed := strings.TrimSpace(string(content))
	if trimmed == "" {
		return errors.New("内容为空")
	}

	// 先按映射解析。Base64 blob / URI 列表会被解析为标量，
	// 在此处即可被明确拒绝。
	var root map[string]any
	if err := yaml.Unmarshal(content, &root); err != nil {
		return fmt.Errorf("不是合法的 YAML 配置: %s", reasonLine(err.Error()))
	}

	// YAML 为空文档（如仅注释）时 root 为 nil
	if root == nil {
		return errors.New("不是合法的 Clash 配置: 顶层为空或仅含注释")
	}

	// 键名按小写归一，与内核解析行为一致
	normalized := make(map[string]any, len(root))
	for key, value := range root {
		normalized[strings.ToLower(key)] = value
	}

	var found []string
	for _, key := range requiredTopLevelKeys {
		value, ok := normalized[key]
		if !ok {
			continue
		}
		if err := checkKind(key, value); err != nil {
			return err
		}
		found = append(found, key)
	}

	if len(found) == 0 {
		return fmt.Errorf("不是合法的 Clash 配置: 缺少 %s 任一顶层字段",
			strings.Join(requiredTopLevelKeys, " / "))
	}
	return nil
}

// checkKind 校验单个顶层字段的类型是否符合内核预期。
func checkKind(key string, value any) error {
	switch key {
	case "proxies", "proxy-groups":
		if _, ok := value.([]any); !ok {
			return fmt.Errorf("不是合法的 Clash 配置: %s 应为%s，实际为%s",
				key, keyExpectedKind[key], kindOf(value))
		}
	case "proxy-providers":
		if _, ok := value.(map[string]any); !ok {
			return fmt.Errorf("不是合法的 Clash 配置: %s 应为%s，实际为%s",
				key, keyExpectedKind[key], kindOf(value))
		}
	}
	return nil
}

// kindOf 返回便于用户理解的值类型描述。
func kindOf(value any) string {
	switch value.(type) {
	case nil:
		return "空值"
	case []any:
		return "列表"
	case map[string]any:
		return "映射"
	case string:
		return "字符串"
	case bool:
		return "布尔值"
	case int, int64, uint64, float64:
		return "数字"
	default:
		return "未知类型"
	}
}

// reasonLine 提取 yaml.v3 错误中最有信息量的一行。
//
// yaml.v3 的错误形如：
//
//	yaml: unmarshal errors:
//	  line 1: cannot unmarshal !!str `c3M6Ly9...` into map[string]interface {}
//
// 首行只是标题，真正的原因在后续行，故取最后一个非空行；单行错误则原样返回。
func reasonLine(s string) string {
	lines := strings.Split(s, "\n")
	for i := len(lines) - 1; i >= 0; i-- {
		if line := strings.TrimSpace(lines[i]); line != "" {
			return line
		}
	}
	return s
}
