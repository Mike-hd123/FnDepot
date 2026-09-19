package subscription

import (
	"fluxor/internal/config"
	"fluxor/internal/configcheck"
	"fluxor/internal/configgen"
	"fmt"
	"os"

	"gopkg.in/yaml.v3"
)

// patchSubscriptionFile 修改订阅节点文件，注入 Fluxor 必需的端口/密钥/DNS 字段。
//
// 采用 YAML 解析改写而非正则拼接：
//   - 订阅名、路径等值可能含 `:`, `,`, `]` 等 YAML 特殊字符，字符串插值会产出
//     内核无法加载的配置（正则替换同样无法正确转义值）
//   - 解析后按顶层键增删，键序与注释由 yaml.Node 保留
//
// 输入必须是合法的 Clash 配置：若对 Base64 编码的 URI 列表等非 YAML 内容改写，
// 会产出损坏文件，故校验失败时直接报错，由调用方终止流程。
func patchSubscriptionFile(filePath string, cfg config.SubscribeConfig) error {
	content, err := os.ReadFile(filePath)
	if err != nil {
		return err
	}

	doc, err := configcheck.ParseValidatedDoc(content)
	if err != nil {
		return fmt.Errorf("订阅文件 %s 无法打补丁: %w", filePath, err)
	}

	// 清理可能冲突的顶层单端口定义，防止重复端口绑定导致内核崩溃
	for _, key := range []string{"port", "socks-port", "redir-port"} {
		doc.Delete(key)
	}

	// 注入 Fluxor 的监听与运行参数
	fields := []struct {
		key   string
		value any
	}{
		{"mixed-port", cfg.ProxyPort},
		{"tproxy-port", cfg.TproxyPort},
		{"external-controller", fmt.Sprintf("0.0.0.0:%d", cfg.PanelPort)},
		{"external-controller-unix", config.CoreSocket},
		{"secret", cfg.PanelSecret},
		{"allow-lan", true},
		{"ipv6", true},
		{"unified-delay", true},
		{"geodata-mode", false},
		{"routing-mark", 255},
		{"external-ui", uiPath(cfg.UIPanel)},
	}
	if cfg.UIPanel == "zashboard" {
		fields = append(fields, struct {
			key   string
			value any
		}{"external-ui-url", zashboardUIRL})
	}

	for _, f := range fields {
		if err := doc.Set(f.key, f.value); err != nil {
			return fmt.Errorf("写入字段 %s 失败: %w", f.key, err)
		}
	}

	// 替换 DNS 块为 Fluxor 的统一配置
	dnsNode, err := parseDNSBlock()
	if err != nil {
		return err
	}
	doc.SetNode("dns", dnsNode)

	out, err := doc.Bytes()
	if err != nil {
		return err
	}
	return os.WriteFile(filePath, out, 0644)
}

// uiPath 返回外部面板的静态文件目录。
func uiPath(uiPanel string) string {
	if uiPanel == "zashboard" {
		return "ui/zash"
	}
	return "ui/meta"
}

const zashboardUIRL = "https://github.com/Zephyruso/zashboard/releases/latest/download/dist-cdn-fonts.zip"

// parseDNSBlock 把 configgen 的 DNS 块解析为 YAML 节点。
func parseDNSBlock() (*yaml.Node, error) {
	var wrapper struct {
		DNS yaml.Node `yaml:"dns"`
	}
	if err := yaml.Unmarshal([]byte(configgen.DnsBlock), &wrapper); err != nil {
		return nil, fmt.Errorf("解析 DNS 块失败: %w", err)
	}
	if wrapper.DNS.Kind == 0 {
		return nil, fmt.Errorf("解析 DNS 块失败: 未得到 dns 节点")
	}
	return &wrapper.DNS, nil
}
