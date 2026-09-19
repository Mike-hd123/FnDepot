package configgen

import (
	"fluxor/internal/config"
	"fluxor/internal/configcheck"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"gopkg.in/yaml.v3"
)

// GenerateConfig 根据订阅配置生成 config.yaml（融合模式）。
//
// 生成策略：模板负责提供基础骨架与规则集，随后用 YAML 解析改写需要按用户配置
// 覆盖的字段。不采用字符串拼接的原因：订阅名等用户输入可能含 `:`, `,`, `]`
// 等 YAML 特殊字符，插值会产出内核无法加载的配置。
func GenerateConfig(cfg config.SubscribeConfig) error {
	// 无订阅时退化为基础配置。
	//
	// 不能继续走「模板 + 规则集」：full 规则集的代理组以 use: [订阅名] 引用 provider
	// （groups_full.go 的 __SUB_NAMES__），订阅为空时会被序列化为 use: []，内核直接
	// 拒绝加载（proxy group：`use` or `proxies` missing），使「删除最后一个订阅并保存」
	// 后重载必然失败。此时也不应再下发规则集——没有节点可供规则分流。
	if len(cfg.Subscriptions) == 0 {
		return GenerateBaseConfig(cfg)
	}

	providers, err := buildProviders(cfg)
	if err != nil {
		return err
	}

	doc, err := buildBaseDocument(cfg, providers)
	if err != nil {
		return err
	}

	// 按规则集追加动态块（rule-providers -> proxy-groups -> rules）
	if err := appendRuleSet(doc, cfg); err != nil {
		return err
	}

	// 替换 DNS 块
	if err := applyDNSBlock(doc); err != nil {
		return err
	}

	return writeConfigTarget(doc)
}

// GenerateBaseConfig 生成基础配置（用于无订阅时）。
//
// 字段集合与 patch.go 给订阅文件注入的「Fluxor 必需字段」保持一致：
// 其中 tproxy-port 必须保留——TProxy 规则会把流量重定向到该端口，若基础配置
// 不声明该端口，内核不再监听，已启用的 TProxy 将把流量导入黑洞。
func GenerateBaseConfig(cfg config.SubscribeConfig) error {
	doc := configcheck.NewDoc()

	fields := []struct {
		key   string
		value any
	}{
		{"mixed-port", cfg.ProxyPort},
		{"tproxy-port", cfg.TproxyPort},
		{"allow-lan", true},
		{"mode", "rule"},
		{"log-level", "silent"},
		{"external-controller-unix", config.CoreSocket},
		{"external-controller", fmt.Sprintf("0.0.0.0:%d", cfg.PanelPort)},
		{"external-ui", uiPath(cfg.UIPanel)},
	}
	if cfg.PanelSecret != "" {
		fields = append(fields, struct {
			key   string
			value any
		}{"secret", cfg.PanelSecret})
	}
	if cfg.UIPanel == "zashboard" {
		fields = append(fields, struct {
			key   string
			value any
		}{"external-ui-url", zashboardUIURL})
	}

	for _, f := range fields {
		if err := doc.Set(f.key, f.value); err != nil {
			return fmt.Errorf("写入字段 %s 失败: %w", f.key, err)
		}
	}
	return writeConfigTarget(doc)
}

// buildBaseDocument 以模板为骨架，注入 Fluxor 的监听参数与 proxy-providers。
func buildBaseDocument(cfg config.SubscribeConfig, providers *yaml.Node) (*configcheck.Doc, error) {
	doc, err := configcheck.ParseDoc([]byte(configTemplate))
	if err != nil {
		return nil, fmt.Errorf("解析配置模板失败: %w", err)
	}

	fields := []struct {
		key   string
		value any
	}{
		{"mixed-port", cfg.ProxyPort},
		{"tproxy-port", cfg.TproxyPort},
		// fork 修复：部署方（fpk/systemd 等）通过 CORE_SOCKET 覆盖后，
		// 内核监听的 unix 路径必须同步写入生成配置，否则面板与内核 socket 对不上。
		{"external-controller-unix", config.CoreSocket},
		{"external-controller", fmt.Sprintf("0.0.0.0:%d", cfg.PanelPort)},
		{"secret", cfg.PanelSecret},
		{"external-ui", uiPath(cfg.UIPanel)},
	}
	if cfg.UIPanel == "zashboard" {
		fields = append(fields, struct {
			key   string
			value any
		}{"external-ui-url", zashboardUIURL})
	}

	for _, f := range fields {
		if err := doc.Set(f.key, f.value); err != nil {
			return nil, fmt.Errorf("写入字段 %s 失败: %w", f.key, err)
		}
	}

	if providers != nil {
		doc.SetNode("proxy-providers", providers)
	}
	return doc, nil
}

// buildProviders 构建 proxy-providers 映射节点。无订阅时返回 nil。
//
// 订阅名作为 YAML 键由节点承载，避免含 `:` 时破坏映射语法。
func buildProviders(cfg config.SubscribeConfig) (*yaml.Node, error) {
	if len(cfg.Subscriptions) == 0 {
		return nil, nil
	}

	providers := &yaml.Node{Kind: yaml.MappingNode, Tag: "!!map"}
	for _, sub := range cfg.Subscriptions {
		interval := sub.UpdateInterval
		if interval <= 0 {
			interval = 86400
		}
		health := sub.HealthInterval
		if health <= 0 {
			health = 300
		}

		provider := &yaml.Node{Kind: yaml.MappingNode, Tag: "!!map"}
		appendMap(provider, "type", "http")
		appendMap(provider, "url", sub.URL)
		appendMap(provider, "interval", interval)
		appendMap(provider, "path", "proxies/"+config.SanitizeSubscriptionFileName(sub.Name))

		healthCheck := &yaml.Node{Kind: yaml.MappingNode, Tag: "!!map"}
		appendMap(healthCheck, "enable", true)
		appendMap(healthCheck, "url", "https://www.gstatic.com/generate_204")
		appendMap(healthCheck, "interval", health)
		appendMapNode(provider, "health-check", healthCheck)

		if sub.Prefix != "" {
			override := &yaml.Node{Kind: yaml.MappingNode, Tag: "!!map"}
			appendMap(override, "additional-prefix", sub.Prefix)
			appendMapNode(provider, "override", override)
		}

		nameNode := &yaml.Node{Kind: yaml.ScalarNode, Tag: "!!str", Value: sub.Name}
		providers.Content = append(providers.Content, nameNode, provider)
	}
	return providers, nil
}

// appendRuleSet 按规则集追加 rule-providers / proxy-groups / rules 三个顶层块。
func appendRuleSet(doc *configcheck.Doc, cfg config.SubscribeConfig) error {
	var blocks []string
	switch cfg.RuleGroup {
	case "base":
		// 1.4.0-1：base 规则集补齐 5 个 smart 地区组，需按订阅名注入 use 列表
		blocks = []string{proxyGroupsBase(cfg.Subscriptions), rulesBase}
	case "full":
		blocks = []string{ruleProvidersFull, proxyGroupsFull(cfg.Subscriptions), rulesFull}
	default:
		return fmt.Errorf("未知规则集: %s", cfg.RuleGroup)
	}

	for _, block := range blocks {
		if strings.TrimSpace(block) == "" {
			continue
		}
		if err := mergeBlock(doc, block); err != nil {
			return err
		}
	}
	return nil
}

// proxyGroupsFull 生成 full 规则集的代理组块。
//
// 订阅名通过 YAML 节点注入 use 列表，而非文本替换 __SUB_NAMES__：
// 订阅名含 `,` / `]` 时，文本替换会破坏 flow sequence 语法导致内核拒绝加载。
func proxyGroupsFull(subs []config.Subscription) string {
	return injectSubNamesIntoGroups(proxyGroupsFullTemplate, subs)
}

// proxyGroupsBase 生成 base 规则集的代理组块（1.4.0-1 起也支持 5 个 smart 地区组）。
//
// 与 proxyGroupsFull 共享同一份订阅名注入逻辑，避免订阅名含特殊字符时破坏 YAML。
func proxyGroupsBase(subs []config.Subscription) string {
	return injectSubNamesIntoGroups(proxyGroupsBaseTemplate, subs)
}

// injectSubNamesIntoGroups 把模板中的 use: [__SUB_NAMES__] 替换为真实订阅名列表。
//
// 用 YAML 节点注入而非文本拼接：订阅名可能含 `,` / `]` 等 flow sequence 特殊字符，
// 直接拼接会破坏语法导致内核拒绝加载配置。
func injectSubNamesIntoGroups(template string, subs []config.Subscription) string {
	doc := configcheck.ParseMappingDoc([]byte(
		strings.ReplaceAll(template, "__SUB_NAMES__", "placeholder")))

	groups := doc.Get("proxy-groups")
	if groups == nil || groups.Kind != yaml.SequenceNode {
		return template
	}

	useNode := &yaml.Node{Kind: yaml.SequenceNode, Tag: "!!seq"}
	for _, s := range subs {
		useNode.Content = append(useNode.Content,
			&yaml.Node{Kind: yaml.ScalarNode, Tag: "!!str", Value: s.Name})
	}

	for _, group := range groups.Content {
		if group.Kind != yaml.MappingNode {
			continue
		}
		for i := 0; i+1 < len(group.Content); i += 2 {
			if group.Content[i].Value == "use" {
				group.Content[i+1] = useNode
			}
			// ASN.mmdb 缺失时把 prefer-asn 降为 false（1.4.0-3 掉线根因防御）。
			// 用 YAML 节点改写而非文本替换，避免 prefer-asn 与其他键相邻时被
			// 误替换，也避免把布尔值写成字符串。
			if group.Content[i].Value == "prefer-asn" {
				group.Content[i+1] = &yaml.Node{
					Kind:  yaml.ScalarNode,
					Tag:   "!!bool",
					Value: strconv.FormatBool(asnAvailable()),
				}
			}
		}
	}

	out, err := doc.Bytes()
	if err != nil {
		return template
	}
	return string(out)
}

// mergeBlock 解析一个 YAML 块并合并进文档：同名顶层键以块内容覆盖。
func mergeBlock(doc *configcheck.Doc, block string) error {
	var parsed yaml.Node
	if err := yaml.Unmarshal([]byte(block), &parsed); err != nil {
		return fmt.Errorf("解析配置块失败: %w", err)
	}
	if len(parsed.Content) == 0 {
		return nil
	}
	blockTop := parsed.Content[0]
	if blockTop.Kind != yaml.MappingNode {
		return fmt.Errorf("解析配置块失败: 顶层应为映射")
	}
	for i := 0; i+1 < len(blockTop.Content); i += 2 {
		doc.SetNode(blockTop.Content[i].Value, blockTop.Content[i+1])
	}
	return nil
}

// applyDNSBlock 把 Fluxor 的统一 DNS 块写入文档。
func applyDNSBlock(doc *configcheck.Doc) error {
	var wrapper struct {
		DNS yaml.Node `yaml:"dns"`
	}
	if err := yaml.Unmarshal([]byte(DnsBlock), &wrapper); err != nil {
		return fmt.Errorf("解析 DNS 块失败: %w", err)
	}
	doc.SetNode("dns", &wrapper.DNS)
	return nil
}

// writeConfigTarget 序列化文档并写入内核配置路径。
func writeConfigTarget(doc *configcheck.Doc) error {
	out, err := doc.Bytes()
	if err != nil {
		return err
	}
	dir := filepath.Dir(config.ConfigTarget)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return fmt.Errorf("创建目录失败: %w", err)
	}
	return os.WriteFile(config.ConfigTarget, out, 0644)
}

// uiPath 返回外部面板的静态文件目录。
func uiPath(uiPanel string) string {
	if uiPanel == "zashboard" {
		return "ui/zash"
	}
	return "ui/meta"
}

const zashboardUIURL = "https://github.com/Zephyruso/zashboard/releases/latest/download/dist-cdn-fonts.zip"

// appendMap 向映射节点追加一个标量键值对。
func appendMap(m *yaml.Node, key string, value any) {
	var v yaml.Node
	_ = v.Encode(value)
	appendMapNode(m, key, &v)
}

// appendMapNode 向映射节点追加一个键值对。
func appendMapNode(m *yaml.Node, key string, value *yaml.Node) {
	m.Content = append(m.Content,
		&yaml.Node{Kind: yaml.ScalarNode, Tag: "!!str", Value: key}, value)
}
