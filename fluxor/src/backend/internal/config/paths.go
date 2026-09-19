package config

var (
	// SocketPath Unix Domain Socket 监听路径；为空表示禁用 Unix Socket 监听。
	SocketPath string
	// BaseURL 所有 HTTP 路由的统一前缀（例如 /app/Fluxor）；为 / 时归一化为空串。
	BaseURL string
	// FluxorPidFile Fluxor 自身进程的 PID 文件路径。
	FluxorPidFile string
	// FluxorBinDir Fluxor 二进制所在目录，自更新时用于备份与替换。
	FluxorBinDir string
	// CorePidFile 内核进程 PID 文件路径，用于判断内核是否在运行。
	CorePidFile string
	// CoreBin Mihomo 内核可执行文件路径。
	CoreBin string
	// CoreSocket 内核暴露的 Unix Socket 路径，Fluxor 经此转发全部内核 API。
	CoreSocket string
	// MetaDir MetaCubeXD 外部面板静态文件目录。
	MetaDir string
	// ZashDir Zashboard 外部面板静态文件目录。
	ZashDir string
	// FluxorConfigFile Fluxor 的 JSON 配置与持久化状态文件。
	FluxorConfigFile string
	// ConfigTarget 生成给内核使用的 config.yaml 目标路径。
	ConfigTarget string
	// InfoLogFile 内核启停等运维操作的日志文件。
	InfoLogFile string
	// CoreWorkDir 内核工作目录，其下 proxies/ 存放各订阅的节点文件。
	CoreWorkDir string
	// TcpAddr 可选的 TCP 监听地址；与 SocketPath 至少有一个非空。
	TcpAddr string
	// OriginalBaseURL 未经归一化处理的原始 BaseURL，注入前端作为 window.BASE_URL。
	OriginalBaseURL string
)
