package config

// SetDefaults 根据运行模式设置默认路径
func SetDefaults(mode string) {
	switch mode {
	case "openwrt":
		SocketPath = ""
		BaseURL = "/"
		TcpAddr = "0.0.0.0:18080"
		FluxorPidFile = "/var/run/fluxor.pid"
		FluxorBinDir = "/etc/fluxor/"
		CorePidFile = "/var/run/core.pid"
		CoreBin = "/etc/fluxor/mihomo"
		CoreSocket = "/etc/fluxor/core.sock"
		MetaDir = "/etc/fluxor/ui/meta"
		ZashDir = "/etc/fluxor/ui/zash"
		FluxorConfigFile = "/etc/fluxor/fluxor.json"
		ConfigTarget = "/etc/fluxor/config.yaml"
		InfoLogFile = "/etc/fluxor/info.log"
		CoreWorkDir = "/etc/fluxor"
	default: // fnos 模式（默认）
		SocketPath = "/var/apps/Fluxor/target/app.sock"
		BaseURL = "/app/Fluxor"
		TcpAddr = ""
		FluxorPidFile = "/var/apps/Fluxor/var/fluxor.pid"
		FluxorBinDir = "/var/apps/Fluxor/target/bin/"
		CorePidFile = "/var/apps/Fluxor/var/core.pid"
		CoreBin = "/var/apps/Fluxor/target/bin/mihomo"
		CoreSocket = "/var/apps/Fluxor/target/core.sock"
		MetaDir = "/var/apps/Fluxor/shares/ui/meta"
		ZashDir = "/var/apps/Fluxor/shares/ui/zash"
		FluxorConfigFile = "/var/apps/Fluxor/var/fluxor.json"
		ConfigTarget = "/var/apps/Fluxor/shares/Fluxor/config.yaml"
		InfoLogFile = "/var/apps/Fluxor/shares/Fluxor/info.log"
		CoreWorkDir = "/var/apps/Fluxor/shares/Fluxor"
	}
	OriginalBaseURL = BaseURL
}
