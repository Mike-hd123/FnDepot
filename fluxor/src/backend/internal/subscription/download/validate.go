package download

import (
	"fluxor/internal/configcheck"
)

// validateSubscriptionContent 校验直连获取的内容是否为可被内核加载的 Clash 配置。
//
// 与临时内核产出侧的校验共用 configcheck，避免两处判定标准漂移：
// 此前这里是「子串匹配 proxies:」的弱校验，既放过了注释里的假字段，
// 也无法识别字段类型错误。
func validateSubscriptionContent(content string) error {
	return configcheck.ValidateClashConfig([]byte(content))
}
