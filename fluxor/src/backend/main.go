package main

import (
	"embed"
	"fluxor/internal/appupdate"
	"fluxor/internal/config"
	"fluxor/internal/configgen"
	"fluxor/internal/core"
	"fluxor/internal/dashapi"
	"fluxor/internal/delaytest"
	"fluxor/internal/netinfo"
	"fluxor/internal/quality"
	"fluxor/internal/subscription"
	"fluxor/internal/tproxy"
	"fluxor/internal/web"
	"fluxor/internal/wsproxy"
	"fmt"
	"html/template"
	"io/fs"
	"net"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"syscall"
)

// staticFS 内嵌前端构建产物 (frontend/dist) 的文件系统根
var staticFS fs.FS

//go:embed dist
var distFS embed.FS

func init() {
	// dist 由 Makefile 在构建前端后同步到 backend/dist，并在此内嵌
	sub, err := fs.Sub(distFS, "dist")
	if err != nil {
		panic(err)
	}
	staticFS = sub
}

func main() {
	// === 解析命令行参数 ===
	openwrtMode := false
	fnosMode := false
	customAddr := ""
	args := os.Args[1:]
	for i := 0; i < len(args); i++ {
		arg := args[i]
		switch arg {
		case "-w", "--openwrt":
			openwrtMode = true
		case "-f", "--fnos":
			fnosMode = true
		case "-a", "--addr":
			if i+1 < len(args) {
				customAddr = args[i+1]
				i++
			} else {
				fmt.Println("错误：-a 或 --addr 需要指定地址")
				os.Exit(1)
			}
		default:
			// 忽略未知参数
		}
	}

	// 确定运行模式：-w 优先，否则若指定 -f 或无参数则 fnos
	mode := "fnos"
	if openwrtMode {
		mode = "openwrt"
	} else if !fnosMode && len(args) > 0 {
		// 若指定了其他参数（如 -a 等）但没有 -f/-w，仍视作 fnos
		// 因此保持 mode = "fnos"
	}

	// 设置默认值（根据模式）
	config.SetDefaults(mode)

	// 环境变量覆盖（所有配置均可通过环境变量修改）
	if v := os.Getenv("SOCKET_PATH"); v != "" {
		config.SocketPath = v
	}
	if v := os.Getenv("BASE_URL"); v != "" {
		config.BaseURL = v
	}
	if v := os.Getenv("FLUXOR_ADDR"); v != "" {
		config.TcpAddr = v
	}
	if v := os.Getenv("FLUXOR_PID_FILE"); v != "" {
		config.FluxorPidFile = v
	}
	if v := os.Getenv("FLUXOR_BIN_DIR"); v != "" {
		config.FluxorBinDir = v
	}
	if v := os.Getenv("CORE_PID_FILE"); v != "" {
		config.CorePidFile = v
	}
	if v := os.Getenv("CORE_BIN"); v != "" {
		config.CoreBin = v
	}
	if v := os.Getenv("CORE_SOCKET"); v != "" {
		config.CoreSocket = v
	}
	if v := os.Getenv("META_DIR"); v != "" {
		config.MetaDir = v
	}
	if v := os.Getenv("ZASH_DIR"); v != "" {
		config.ZashDir = v
	}
	if v := os.Getenv("FLUXOR_CONFIG_FILE"); v != "" {
		config.FluxorConfigFile = v
	}
	if v := os.Getenv("CONFIG_TARGET"); v != "" {
		config.ConfigTarget = v
	}
	if v := os.Getenv("INFO_LOG_FILE"); v != "" {
		config.InfoLogFile = v
	}
	if v := os.Getenv("CORE_WORK_DIR"); v != "" {
		config.CoreWorkDir = v
	}

	// 命令行 -a 覆盖 TCP 地址（最高优先级）
	if customAddr != "" {
		config.TcpAddr = customAddr
	}

	// 更新 originalBaseURL（可能被环境变量修改）
	config.OriginalBaseURL = config.BaseURL

	if mode == "openwrt" {
		fmt.Printf("Fluxor 运行于 OpenWrt 模式")
	}

	// === 检查和准备 ===
	config.LoadSubscribeConfig()
	core.InitCoreLogger()
	subscription.StartAllTimers()
	tproxy.LoadTproxySrcExceptions()
	tproxy.LoadTproxyDstExceptions()
	tproxy.LoadTproxyProxyLocal()
	tproxy.LoadTproxyDNSRedirectPort()
	// 冷启动收敛：清除上次非优雅退出可能残留的 nft/策略路由规则，再按磁盘
	// 状态收敛开关——首次安装默认开启并落盘，重启则为开时重装规则、为关时保持关闭。
	//
	// 注意（防回退约定）：fluxor 只操作 nft/策略路由层面的透明代理，绝不设置
	// 系统级代理（http_proxy/https_proxy 环境变量、gsettings、NetworkManager、
	// uci 等）。系统代理默认不设置，由用户按需自行配置；新增代码不得引入
	// 任何形式的系统代理写操作。
	tproxy.ResetOnStartup(config.Current.TproxyPort)
	// 清理上次非优雅退出遗留的临时内核进程与临时文件
	core.CleanupStaleTempCores()

	if _, err := os.Stat(config.ConfigTarget); os.IsNotExist(err) {
		if err := configgen.GenerateConfig(config.Current); err != nil {
			fmt.Printf("生成基本配置文件失败: %v\n", err)
		} else {
			fmt.Println("已生成基本配置文件 (config.yaml)")
		}
	}

	if err := os.MkdirAll(filepath.Dir(config.FluxorPidFile), 0755); err != nil {
		fmt.Printf("无法创建 PID 目录: %v\n", err)
	} else {
		pidData := []byte(fmt.Sprintf("%d", os.Getpid()))
		if err := os.WriteFile(config.FluxorPidFile, pidData, 0644); err != nil {
			fmt.Printf("写入 PID 文件失败: %v\n", err)
		} else {
			defer func() {
				if err := os.Remove(config.FluxorPidFile); err != nil {
					fmt.Printf("删除 PID 文件失败: %v\n", err)
				}
			}()
		}
	}

	var err error
	web.IndexTmpl, err = template.ParseFS(staticFS, "index.html")
	if err != nil {
		fmt.Printf("加载主页模板失败: %v\n", err)
		os.Exit(1)
	}

	// === 检查监听方式 ===
	if config.SocketPath == "" && config.TcpAddr == "" {
		fmt.Println("错误：未配置任何监听地址（SOCKET_PATH 和 FLUXOR_ADDR 均为空）")
		os.Exit(1)
	}

	// === 创建 Unix socket 监听器（若启用）===
	var listener net.Listener
	if config.SocketPath != "" {
		if err := os.MkdirAll(filepath.Dir(config.SocketPath), 0755); err != nil {
			fmt.Printf("无法创建 socket 目录: %v\n", err)
			os.Exit(1)
		}
		os.Remove(config.SocketPath)

		listener, err = net.Listen("unix", config.SocketPath)
		if err != nil {
			fmt.Printf("监听 Unix socket 失败: %v\n", err)
			os.Exit(1)
		}
		defer listener.Close()

		// 0660：仅属主与所属组可读写，收窄此前 0666（任意本地用户均可
		// 通过该 socket 全权操作内核）。
		if err := os.Chmod(config.SocketPath, 0660); err != nil {
			fmt.Printf("设置 socket 权限失败: %v\n", err)
		}
		fmt.Printf("Unix socket 监听: %s\n", config.SocketPath)
	} else {
		fmt.Println("Unix socket 已禁用")
	}

	// === 创建 TCP 监听器（若启用）===
	var tcpListener net.Listener
	if config.TcpAddr != "" {
		if err := netinfo.ValidateTCPAddr(config.TcpAddr); err != nil {
			fmt.Printf("无效的 FLUXOR_ADDR 格式: %v，将禁用 TCP 监听\n", err)
			config.TcpAddr = ""
		}
		if config.TcpAddr != "" {
			tcpListener, err = net.Listen("tcp", config.TcpAddr)
			if err != nil {
				fmt.Printf("无法监听 TCP 地址 %s: %v\n", config.TcpAddr, err)
			} else {
				defer tcpListener.Close()
				fmt.Printf("TCP 监听: %s\n", config.TcpAddr)
			}
		}
	}

	if config.BaseURL == "/" {
		config.BaseURL = ""
	} else {
		config.BaseURL = strings.TrimSuffix(config.BaseURL, "/")
	}

	// === 创建路由 ===
	mux := http.NewServeMux()

	// 外部静态面板
	mux.Handle(config.BaseURL+"/meta/", http.StripPrefix(config.BaseURL+"/meta/", http.FileServer(http.Dir(config.MetaDir))))
	mux.Handle(config.BaseURL+"/zash/", http.StripPrefix(config.BaseURL+"/zash/", http.FileServer(http.Dir(config.ZashDir))))

	// 内嵌静态文件（Vue 构建产物 assets/ 目录，直接挂载在 baseURL 下）
	staticFileServer := http.FileServer(http.FS(staticFS))
	mux.Handle(config.BaseURL+"/assets/", http.StripPrefix(config.BaseURL, staticFileServer))
	// 内嵌静态根文件（index.html 之外的静态资源，如 favicon ICON.PNG）
	mux.Handle(config.BaseURL+"/ICON.PNG", http.StripPrefix(config.BaseURL, staticFileServer))

	// 页面路由
	if config.BaseURL == "" {
		// 根路径直接渲染首页
		mux.HandleFunc("/", web.HandleIndex)
	} else {
		// 根路径重定向到实际前缀
		mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
			if r.URL.Path == "/" {
				redirectTo := config.BaseURL
				if !strings.HasSuffix(redirectTo, "/") {
					redirectTo += "/"
				}
				http.Redirect(w, r, redirectTo, http.StatusFound)
				return
			}
			http.NotFound(w, r)
		})
		// 实际页面路由
		mux.HandleFunc(config.BaseURL+"/", web.HandleIndex)
	}
	mux.HandleFunc(config.BaseURL+"/whoami", web.HandleWhoAmI)
	mux.HandleFunc(config.BaseURL+"/app-version", web.HandleAppVersion)

	// 内核控制
	mux.HandleFunc(config.BaseURL+"/core/status", core.HandleCoreStatus)
	// 内核状态变更的 SSE 推送（前端据此替代轮询）
	mux.HandleFunc(config.BaseURL+"/core/events", core.HandleCoreEvents)
	mux.HandleFunc(config.BaseURL+"/core/start", core.HandleCoreStart)
	mux.HandleFunc(config.BaseURL+"/core/stop", core.HandleCoreStop)
	mux.HandleFunc(config.BaseURL+"/core/restart", core.HandleCoreRestart)
	mux.HandleFunc(config.BaseURL+"/upgrade", appupdate.HandleUpgradeGate) // smart 内核闸门：拦降级，其余透传
	mux.HandleFunc(config.BaseURL+"/core/smart-update", appupdate.HandleSmartCoreUpdate)
	mux.HandleFunc(config.BaseURL+"/core/check-update", appupdate.HandleCoreCheckUpdate)

	// 订阅中心 API
	mux.HandleFunc(config.BaseURL+"/subscribe/config", subscription.HandleSubscribeConfigAPI)
	mux.HandleFunc(config.BaseURL+"/subscribe/generate", subscription.HandleGenerateConfig)
	mux.HandleFunc(config.BaseURL+"/subscribe/update/", subscription.HandleSubscribeUpdate)
	mux.HandleFunc(config.BaseURL+"/subscribe/update-info/", subscription.HandleUpdateSubscriptionInfo)

	// 获取所有订阅的代理信息（融合模式使用）
	mux.HandleFunc(config.BaseURL+"/providers/proxies", dashapi.HandleProvidersProxiesAll)
	mux.HandleFunc(config.BaseURL+"/providers/proxies/", dashapi.HandleProviderProxies)

	// 策略组测速（组内所有节点/子策略组）
	mux.HandleFunc(config.BaseURL+"/group/", dashapi.HandleGroupDelay)

	// WebSocket 代理
	mux.HandleFunc(config.BaseURL+"/traffic", wsproxy.WsProxyHandler("/traffic"))
	mux.HandleFunc(config.BaseURL+"/memory", wsproxy.WsProxyHandler("/memory"))

	// HTTP 代理
	mux.HandleFunc(config.BaseURL+"/version", dashapi.HandleVersion)
	mux.HandleFunc(config.BaseURL+"/configs", dashapi.HandleConfigsAPI)
	mux.HandleFunc(config.BaseURL+"/interfaces", netinfo.HandleInterfaces)
	mux.HandleFunc(config.BaseURL+"/configs/geo", dashapi.HandleConfigsGeo)
	mux.HandleFunc(config.BaseURL+"/providers/geo", dashapi.HandleProvidersGeo)
	mux.HandleFunc(config.BaseURL+"/cache/fakeip/flush", dashapi.HandleFlushFakeIP)
	mux.HandleFunc(config.BaseURL+"/cache/dns/flush", dashapi.HandleFlushDNS)
	mux.HandleFunc(config.BaseURL+"/dns/query", dashapi.HandleDNSQuery)
	mux.HandleFunc(config.BaseURL+"/restart", dashapi.HandleRestart)
	mux.HandleFunc(config.BaseURL+"/config/tproxy", tproxy.HandleTproxyState)
	mux.HandleFunc(config.BaseURL+"/config/tproxy/exceptions", tproxy.HandleTproxyExceptions)
	mux.HandleFunc(config.BaseURL+"/config/tproxy/proxy-local", tproxy.HandleTproxyProxyLocal)
	mux.HandleFunc(config.BaseURL+"/config/tproxy/dns-port", tproxy.HandleTproxyDNSRedirectPort)

	mux.HandleFunc(config.BaseURL+"/ipinfo/local/v4", netinfo.HandleLocalIPv4)
	mux.HandleFunc(config.BaseURL+"/ipinfo/local/v6", netinfo.HandleLocalIPv6)
	mux.HandleFunc(config.BaseURL+"/ipinfo/proxy/v4", netinfo.HandleProxyIPv4)
	mux.HandleFunc(config.BaseURL+"/ipinfo/proxy/v6", netinfo.HandleProxyIPv6)

	mux.HandleFunc(config.BaseURL+"/delaytest/google", delaytest.HandleDelayTestGoogle)
	mux.HandleFunc(config.BaseURL+"/delaytest/youtube", delaytest.HandleDelayTestYouTube)
	mux.HandleFunc(config.BaseURL+"/delaytest/github", delaytest.HandleDelayTestGitHub)
	mux.HandleFunc(config.BaseURL+"/delaytest/baidu", delaytest.HandleDelayTestBaidu)
	mux.HandleFunc(config.BaseURL+"/delaytest/bilibili", delaytest.HandleDelayTestBilibili)
	mux.HandleFunc(config.BaseURL+"/delaytest/custom", delaytest.HandleDelayTestCustom)

	// 代理 API
	mux.HandleFunc(config.BaseURL+"/proxies", dashapi.HandleProxies)
	mux.HandleFunc(config.BaseURL+"/proxies/", func(w http.ResponseWriter, r *http.Request) {
		if strings.HasSuffix(r.URL.Path, "/delay") || strings.Contains(r.URL.Path, "/delay?") {
			dashapi.HandleProxyDelay(w, r)
		} else {
			dashapi.HandleProxySwitch(w, r)
		}
	})

	// fluxor 版本更新
	mux.HandleFunc(config.BaseURL+"/check-update", appupdate.HandleCheckUpdate)
	mux.HandleFunc(config.BaseURL+"/update-self", appupdate.HandleSelfUpdate)

	// 质量分数
	mux.HandleFunc(config.BaseURL+"/proxies/quality", quality.HandleQualityScores)

	// 日志 WebSocket
	mux.HandleFunc(config.BaseURL+"/logs", wsproxy.WsProxyHandler("/logs"))

	// 规则 API
	mux.HandleFunc(config.BaseURL+"/rules", dashapi.HandleRules)
	mux.HandleFunc(config.BaseURL+"/rules/disable", dashapi.HandleRulesDisable)
	mux.HandleFunc(config.BaseURL+"/providers/rules", dashapi.HandleRuleProviders)
	mux.HandleFunc(config.BaseURL+"/providers/rules/", dashapi.HandleUpdateRuleProvider)

	// 连接管理
	mux.HandleFunc(config.BaseURL+"/connections", func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodDelete {
			dashapi.HandleConnectionsClose(w, r)
		} else {
			wsproxy.WsProxyHandler("/connections")(w, r)
		}
	})
	mux.HandleFunc(config.BaseURL+"/connections/", func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodDelete {
			dashapi.HandleConnectionsClose(w, r)
		} else {
			http.Error(w, "Method Not Allowed", http.StatusMethodNotAllowed)
		}
	})

	// 自动启动内核
	if !core.IsCoreRunning() {
		if err := core.StartCore(); err != nil {
			fmt.Printf("自动启动内核失败: %v\n", err)
		}
	} else {
		fmt.Println("内核已在运行，跳过自动启动")
	}

	// === 网关前缀双兼容 ===
	// fnOS 统一网关（gatewayPrefix=/app/fluxor）把 /app/fluxor/xxx 原样转发到 socket
	// （不剥前缀），而本地 TCP 直连（127.0.0.1:18099/xxx）不带前缀。fluxor 的路由注册在
	// config.BaseURL（生产为 /，归一化为空串）下，即 root 路径。因此对带网关前缀的请求：
	//   (1) 把前缀记进 ctx，供 HandleIndex 注入正确的 <base href> 与 window.BASE_URL，
	//       否则子资源（/assets/…）会丢掉前缀而 404；
	//   (2) 剥掉前缀再进 mux，使 root 路由能匹配。
	// 不带前缀（TCP 直连）的请求原样透传，ctx 前缀为空 → HandleIndex 按 config.BaseURL 注入。
	gatewayPrefix := os.Getenv("FLUXOR_GATEWAY_PREFIX")
	if gatewayPrefix == "" {
		gatewayPrefix = "/app/fluxor"
	}
	stripGatewayPrefix := func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if gatewayPrefix != "/" && strings.HasPrefix(r.URL.Path, gatewayPrefix) {
				rest := strings.TrimPrefix(r.URL.Path, gatewayPrefix)
				if rest == "" {
					rest = "/"
				}
				r = r.Clone(r.Context())
				r.URL.Path = rest
				if r.URL.RawPath != "" {
					rp := strings.TrimPrefix(r.URL.RawPath, gatewayPrefix)
					if rp == "" {
						rp = "/"
					}
					r.URL.RawPath = rp
				}
				r = r.WithContext(web.WithGatewayPrefix(r.Context(), gatewayPrefix))
			}
			next.ServeHTTP(w, r)
		})
	}
	handler := stripGatewayPrefix(mux)

	// 无条件发布一次初态，确保 SSE hub 的状态「已确定」。
	//
	// 这一步是必需的：hub 的「接入即快照」依赖它已知道当前状态，否则
	// 新订阅者接入后收不到任何事件——而 /core/events 是前端启动时获取
	// 内核状态的唯一来源（启动阶段不再请求 /core/status）。
	// 内核确实在跑时（StartCore 内部已发布一次）此调用因状态未变化而成为空操作。
	core.PublishCoreState(core.IsCoreRunning())

	// === 启动服务 ===
	if listener != nil {
		go func() {
			err := http.Serve(listener, handler)
			if err != nil && !strings.Contains(err.Error(), "use of closed network connection") {
				fmt.Printf("Unix HTTP 服务错误: %v\n", err)
			}
		}()
	}
	if tcpListener != nil {
		go func() {
			fmt.Printf("Fluxor TCP 服务已启动，监听: %s\n", config.TcpAddr)
			if err := http.Serve(tcpListener, handler); err != nil && !strings.Contains(err.Error(), "use of closed network connection") {
				fmt.Printf("TCP HTTP 服务错误: %v\n", err)
			}
		}()
	}

	// 等待退出信号
	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	<-quit

	fmt.Printf("收到退出信号，正在关闭 Fluxor...\n")
	subscription.StopAllTimers()
	tproxy.DisableTProxyRules()
	if core.IsCoreRunning() {
		if err := core.StopCore(); err != nil {
			fmt.Printf("停止内核失败: %v\n", err)
		}
	} else {
		fmt.Printf("内核未运行，无需停止\n")
	}
	fmt.Printf("Fluxor 已安全退出\n")
}
