package download

import (
	"strings"
)

// NormalizeMapKeys 将 map 中所有字符串键转换为小写
func NormalizeMapKeys(m map[string]interface{}) map[string]interface{} {
	if m == nil {
		return nil
	}
	result := make(map[string]interface{})
	for k, v := range m {
		result[strings.ToLower(k)] = v
	}
	return result
}
