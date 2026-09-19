package tproxy

import (
	"encoding/json"
	"fluxor/internal/config"
	"log"
)

// fluxor.json 中由本包负责的字段。
//
// 这些字段与订阅配置共用同一个文件，因此写入一律通过 config.UpdateConfigFile
// 走「读—改—写」，与 config.SaveSubscribeConfig 共用同一把文件锁（config.FileMu）。
// 切勿在此直接 os.WriteFile 整个文件：那会把订阅配置字段抹掉。
const (
	keyTproxyEnabled       = "tproxy_enabled"
	keyTproxyProxyLocal    = "tproxy_proxy_local"
	keyTproxyDstExceptions = "tproxy_dst_exceptions"
	keyTproxySrcExceptions = "tproxy_src_exceptions"
	keyTproxyExceptionsOld = "tproxy_exceptions" // 旧字段，读取时迁移
)

// 默认例外列表（文件缺失或字段不存在时使用）
func defaultDstExceptions() []string {
	return []string{"# 公共 DNS 服务器", "223.5.5.5 #注释可单独一行也可写在规则后", "1.12.12.12", "# stun服务器", "141.101.90.1"}
}

func defaultSrcExceptions() []string {
	return []string{"# Docker 默认网段", "172.17.0.0/16"}
}

// readStringSliceField 从配置文件的顶层映射中读取一个字符串数组字段。
func readStringSliceField(full map[string]any, key string) ([]string, bool) {
	raw, ok := full[key]
	if !ok {
		return nil, false
	}
	data, err := json.Marshal(raw)
	if err != nil {
		return nil, false
	}
	var out []string
	if err := json.Unmarshal(data, &out); err != nil {
		return nil, false
	}
	return out, true
}

// LoadTproxyDstExceptions 加载目的例外，字段不存在时写入默认值
func LoadTproxyDstExceptions() []string {
	exceptionsMu.Lock()
	defer exceptionsMu.Unlock()

	full, err := config.ReadConfigFile()
	if err == nil {
		// 新字段优先
		if dst, ok := readStringSliceField(full, keyTproxyDstExceptions); ok {
			tproxyDstExceptionsCache = dst
			return dst
		}
		// 回退到旧字段并迁移
		if dst, ok := readStringSliceField(full, keyTproxyExceptionsOld); ok {
			tproxyDstExceptionsCache = dst
			if err := saveDstExceptions(dst); err != nil {
				log.Printf("[TProxy] 迁移目的例外失败: %v", err)
			}
			return dst
		}
	}

	// 文件缺失、损坏或字段不存在：落盘默认值，并同步缓存
	// （此前该分支只 return 默认值而不设置缓存，会让面板读到空列表）
	dst := defaultDstExceptions()
	tproxyDstExceptionsCache = dst
	if err := saveDstExceptions(dst); err != nil {
		log.Printf("[TProxy] 写入默认目的例外失败: %v", err)
	}
	return dst
}

// saveDstExceptions 保存目的例外并移除旧字段。
func saveDstExceptions(dst []string) error {
	return config.UpdateConfigFile(func(full map[string]any) {
		full[keyTproxyDstExceptions] = dst
		delete(full, keyTproxyExceptionsOld)
	})
}

// SaveTproxyDstExceptions 供外部调用（加锁）
func SaveTproxyDstExceptions(dst []string) error {
	exceptionsMu.Lock()
	defer exceptionsMu.Unlock()
	tproxyDstExceptionsCache = dst
	return saveDstExceptions(dst)
}

// LoadTproxySrcExceptions 加载源例外，字段不存在时写入默认值
func LoadTproxySrcExceptions() []string {
	exceptionsMu.Lock()
	defer exceptionsMu.Unlock()

	full, err := config.ReadConfigFile()
	if err == nil {
		if src, ok := readStringSliceField(full, keyTproxySrcExceptions); ok {
			tproxySrcExceptionsCache = src
			return src
		}
	}

	src := defaultSrcExceptions()
	tproxySrcExceptionsCache = src
	if err := saveSrcExceptions(src); err != nil {
		log.Printf("[TProxy] 写入默认源例外失败: %v", err)
	}
	return src
}

func saveSrcExceptions(src []string) error {
	return config.UpdateConfigFile(func(full map[string]any) {
		full[keyTproxySrcExceptions] = src
	})
}

// SaveTproxySrcExceptions 保存源例外列表（加锁），由 HTTP 层调用。
func SaveTproxySrcExceptions(src []string) error {
	exceptionsMu.Lock()
	defer exceptionsMu.Unlock()
	tproxySrcExceptionsCache = src
	return saveSrcExceptions(src)
}

// LoadTproxyProxyLocal 读取 tproxy_proxy_local 字段，默认 true
func LoadTproxyProxyLocal() bool {
	exceptionsMu.Lock()
	defer exceptionsMu.Unlock()

	full, err := config.ReadConfigFile()
	if err == nil {
		if raw, ok := full[keyTproxyProxyLocal]; ok {
			if enabled, ok := raw.(bool); ok {
				tproxyProxyLocal = enabled
				return enabled
			}
		}
	}

	// 缺失或类型异常：默认开启并落盘
	tproxyProxyLocal = true
	if err := saveProxyLocal(true); err != nil {
		log.Printf("[TProxy] 写入默认本机代理开关失败: %v", err)
	}
	return true
}

func saveProxyLocal(enabled bool) error {
	return config.UpdateConfigFile(func(full map[string]any) {
		full[keyTproxyProxyLocal] = enabled
	})
}

// SaveTproxyProxyLocal 外部调用，加锁并保存
func SaveTproxyProxyLocal(enabled bool) error {
	exceptionsMu.Lock()
	defer exceptionsMu.Unlock()
	tproxyProxyLocal = enabled
	return saveProxyLocal(enabled)
}

// LoadTproxyEnabled 读取持久化的 TProxy 开关状态。
//
// 键不存在时返回 false（与 exists=false 组合可区分「从未设置」与「设置为关」）。
func LoadTproxyEnabled() bool {
	enabled, _ := loadTproxyEnabledState()
	return enabled
}

// loadTproxyEnabledState 读取 tproxy_enabled 键的值与存在性。
//
// exists=false 表示该键从未被写入过（全新安装）；读取失败按不存在处理，
// 使损坏配置回落到「首次初始化」路径而非误恢复上次的开启状态。
func loadTproxyEnabledState() (enabled bool, exists bool) {
	full, err := config.ReadConfigFile()
	if err != nil {
		return false, false
	}
	raw, ok := full[keyTproxyEnabled]
	if !ok {
		return false, false
	}
	enabled, _ = raw.(bool)
	return enabled, true
}

// persistTproxyEnabled 持久化开关状态。
func persistTproxyEnabled(enabled bool) error {
	return config.UpdateConfigFile(func(full map[string]any) {
		full[keyTproxyEnabled] = enabled
	})
}

// SetTproxyEnabled 设置内存中的开关状态并持久化，供 HTTP 层复用。
func SetTproxyEnabled(enabled bool) {
	tproxyMu.Lock()
	tproxyEnableState = enabled
	tproxyMu.Unlock()

	if err := persistTproxyEnabled(enabled); err != nil {
		log.Printf("[TProxy] 持久化开关状态失败: %v", err)
	}
}

// 开机路径对规则安装的引用（测试注入 seam）。
//
// 生产环境指向真实实现；单测替换为桩，避免 CI / 无特权环境真的执行
// nft / ip 命令改宿主机网络配置。
var (
	enableRulesFn  = EnableTProxyRules
	disableRulesFn = DisableTProxyRules
	setStateFn     = SetTproxyEnabled
)

// enableTProxyOnStartup 开机装规则并把状态持久化为开。
//
// fail-safe：端口非法（<=0）时不装规则、状态回退为关并明确记日志，
// 避免「面板显示开、流量实际无人接管」的静默错配。
func enableTProxyOnStartup(port int) {
	if port <= 0 {
		log.Printf("[TProxy] 开机启用被跳过：TProxy 端口为 %d（非法），状态保持关闭，请在设置端口后手动开启", port)
		setStateFn(false)
		return
	}
	disableRulesFn() // 幂等：先清掉任何残留再装
	if err := enableRulesFn(port); err != nil {
		log.Printf("[TProxy] 开机启用失败: %v，状态回退为关闭", err)
		setStateFn(false)
		return
	}
	setStateFn(true)
	log.Printf("[TProxy] 已按持久化状态在开机时自动启用（端口 %d）", port)
}

// ResetOnStartup 冷启动时的状态收敛。
//
// nftables/策略路由规则不跨重启存活，而开关状态是持久化的，因此启动时
// 先无条件清残留（上次 kill -9 / 崩溃留下的规则），再按磁盘状态收敛：
//
//   - 首次安装（tproxy_enabled 键从未写入）→ 默认开启并落盘（1.4.1-1 起产品默认）；
//   - 持久化为开 → 重新装规则（规则不跨重启，必须重装才能兑现面板显示）；
//   - 持久化为关 → 保持关闭（清残留后即正确状态）。
//
// 三种分支收敛后内存态、磁盘态与内核态三者一致，不会出现
// 「面板显示关闭、流量仍被劫持」或「面板显示开启、实际没生效」的错配。
func ResetOnStartup(port int) {
	// 先清残留规则（不依赖当前布尔值，确保任何残留都被移除）
	disableRulesFn()

	enabled, exists := loadTproxyEnabledState()
	switch {
	case !exists:
		// 首次安装：默认开启，走 UpdateConfigFile 持久化而非仅内存
		log.Printf("[TProxy] 首次初始化：默认开启透明代理")
		enableTProxyOnStartup(port)
	case enabled:
		// 重启且上次为开：重装规则以恢复实际生效
		enableTProxyOnStartup(port)
	default:
		// 重启且上次为关：保持关闭
		setStateFn(false)
	}
}
