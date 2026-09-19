package config

import (
	"encoding/json"
	"log"
	"os"
	"path/filepath"
	"sync"
)

// FileMu 保护 FluxorConfigFile 的整文件读—改—写。
//
// fluxor.json 同时承载订阅配置（本包的 Current）与 TProxy 相关字段
// （tproxy_enabled / tproxy_dst_exceptions / tproxy_src_exceptions 等）。
// 二者由 config 与 tproxy 两个包分别写入同一个文件，因此必须共用同一把锁，
// 并统一采用「读—改—写」：任何一方若整文件覆写，都会把对方的字段抹掉
// （曾导致「保存一次订阅配置，TProxy 例外列表被静默清空」）。
//
// 锁序约定：FileMu 永远是最内层——持有 config.Mu 或 tproxy 的 exceptionsMu
// 时可以再取 FileMu，反之不可。
var FileMu sync.Mutex

// UpdateConfigFile 在共用文件锁下对 FluxorConfigFile 做一次「读—改—写」。
//
// mutate 收到文件当前的完整顶层映射（文件缺失或损坏时为空映射），就地修改后
// 由本函数序列化写回；mutate 未触碰的键一律保留原值。
func UpdateConfigFile(mutate func(full map[string]any)) error {
	FileMu.Lock()
	defer FileMu.Unlock()

	full := make(map[string]any)
	data, err := os.ReadFile(FluxorConfigFile)
	if err != nil && !os.IsNotExist(err) {
		return err
	}
	if len(data) > 0 {
		// 解析失败时从空映射重建，避免以一份已损坏的内容为基底继续写
		if err := json.Unmarshal(data, &full); err != nil {
			full = make(map[string]any)
		}
	}

	mutate(full)

	dir := filepath.Dir(FluxorConfigFile)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return err
	}
	out, err := json.MarshalIndent(full, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(FluxorConfigFile, out, 0644)
}

// ReadConfigFile 在共用文件锁下读取配置文件的顶层映射快照。
//
// 供 tproxy 等「旁路」字段的读取方使用，与 UpdateConfigFile 共用同一把锁，
// 保证读到的是某次完整写入后的状态而非写入中途的内容。
func ReadConfigFile() (map[string]any, error) {
	FileMu.Lock()
	defer FileMu.Unlock()

	full := make(map[string]any)
	data, err := os.ReadFile(FluxorConfigFile)
	if err != nil {
		return full, err
	}
	if err := json.Unmarshal(data, &full); err != nil {
		return make(map[string]any), err
	}
	return full, nil
}

// defaultSubscribeConfig 返回 SubscribeConfig 的出厂默认值。
//
// 集中定义，避免 LoadSubscribeConfig（启动加载）与其它调用方（如订阅生成时的
// 增量合并）各自硬编码一套默认值而漂移。
func defaultSubscribeConfig() SubscribeConfig {
	return SubscribeConfig{
		ProxyPort:      7890,
		PanelPort:      9090,
		PanelSecret:    "",
		RuleGroup:      "base",
		UIPanel:        "metacubexd",
		MetaBackendURL: "",
		Subscriptions:  []Subscription{},
		TproxyPort:     7898,
	}
}

// WithDefaults 返回 cfg 的副本，仅把未设置（零值）的字段补为出厂默认值。
//
// 与 LoadSubscribeConfig 的「键不存在才补默认」语义一致：已显式设置的值
// （包括用户有意填的 0）一律保留。供 generate 等需要「增量合并后生成配置」
// 的调用方兜底，避免一份不完整的请求体产出端口全 0、无规则集的空壳配置。
func (cfg SubscribeConfig) WithDefaults() SubscribeConfig {
	def := defaultSubscribeConfig()
	if cfg.ProxyPort == 0 {
		cfg.ProxyPort = def.ProxyPort
	}
	if cfg.TproxyPort == 0 {
		cfg.TproxyPort = def.TproxyPort
	}
	if cfg.PanelPort == 0 {
		cfg.PanelPort = def.PanelPort
	}
	if cfg.RuleGroup == "" {
		cfg.RuleGroup = def.RuleGroup
	}
	if cfg.UIPanel == "" {
		cfg.UIPanel = def.UIPanel
	}
	if cfg.Mode == "" {
		cfg.Mode = "merge"
	}
	if cfg.Subscriptions == nil {
		cfg.Subscriptions = []Subscription{}
	}
	return cfg
}

// LoadSubscribeConfig 从文件加载订阅配置（启动时调用），若失败则设置默认值
func LoadSubscribeConfig() {
	Mu.Lock()
	defer Mu.Unlock()

	defaultCfg := defaultSubscribeConfig()

	data, err := os.ReadFile(FluxorConfigFile)
	if err != nil {
		if !os.IsNotExist(err) {
			log.Printf("读取订阅配置失败: %v", err)
		}
		Current = defaultCfg
		return
	}

	var tmp SubscribeConfig
	if err := json.Unmarshal(data, &tmp); err != nil {
		log.Printf("解析订阅配置失败: %v，使用默认配置", err)
		Current = defaultCfg
		return
	}

	// 检查 JSON 中哪些键实际存在
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(data, &raw); err != nil {
		Current = defaultCfg
		return
	}

	// 仅当键不存在时，才使用默认值（避免覆盖用户设置的 0）
	if _, ok := raw["proxy_port"]; !ok {
		tmp.ProxyPort = defaultCfg.ProxyPort
	}
	if _, ok := raw["panel_port"]; !ok {
		tmp.PanelPort = defaultCfg.PanelPort
	}
	if _, ok := raw["tproxy_port"]; !ok {
		tmp.TproxyPort = defaultCfg.TproxyPort
	}

	// 字符串类型字段：若为空则设为默认值（合理）
	if tmp.UIPanel == "" {
		tmp.UIPanel = defaultCfg.UIPanel
	}
	if tmp.Mode == "" {
		tmp.Mode = "merge"
	}
	if tmp.Subscriptions == nil {
		tmp.Subscriptions = []Subscription{}
	}

	Current = tmp
	log.Printf("成功加载订阅配置：%d 个订阅", len(Current.Subscriptions))
}

// SaveSubscribeConfig 保存订阅配置到文件。
//
// 只写入 SubscribeConfig 自身承载的键，其余键（如 tproxy_*）由 UpdateConfigFile
// 原样保留，因此本函数不会再覆盖 tproxy 包写入的字段。
func SaveSubscribeConfig() error {
	// 先在锁内取快照并序列化到 map，避免在持有 FileMu 时再取 config.Mu
	Mu.Lock()
	payload, err := json.Marshal(Current)
	Mu.Unlock()
	if err != nil {
		return err
	}

	var updates map[string]any
	if err := json.Unmarshal(payload, &updates); err != nil {
		return err
	}

	return UpdateConfigFile(func(full map[string]any) {
		for k, v := range updates {
			full[k] = v
		}
	})
}
