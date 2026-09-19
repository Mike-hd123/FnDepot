package tproxy

import (
	"bytes"
	"fmt"
	"log"
	"net"
	"os/exec"
	"strconv"
	"strings"
)

// FWMARK / TABLE_ID 与 EnableTProxyRules 中写入策略路由时使用的取值一致。
const (
	tproxyFwmark  = "1"
	tproxyTableID = "100"
)

// hasFwmarkRule 检测策略路由规则（fwmark 1 -> table 100）是否已存在。
//
// 先探测再删除，避免对不存在的规则执行 del 而徒增错误。
func hasFwmarkRule() bool {
	out, err := exec.Command("ip", "rule", "show").Output()
	if err != nil {
		return false
	}
	return matchFwmarkRule(string(out))
}

// matchFwmarkRule 解析 `ip rule show` 的输出，判断是否含目标策略路由。
// 抽成纯函数以便单测覆盖，无需真实改动系统防火墙。
func matchFwmarkRule(out string) bool {
	for _, line := range strings.Split(out, "\n") {
		if !strings.Contains(line, "fwmark") {
			continue
		}
		if strings.Contains(line, "lookup "+tproxyTableID) || strings.Contains(line, "table "+tproxyTableID) {
			return true
		}
	}
	return false
}

// hasLocalRoute 检测 table 100 中是否存在本地路由（local default dev lo）。
//
// table 不存在时 `ip route show table 100` 会以非零码退出，据此判定为无残留，
// 不执行任何删除。
func hasLocalRoute() bool {
	out, err := exec.Command("ip", "route", "show", "table", tproxyTableID).Output()
	if err != nil {
		return false
	}
	return matchLocalRoute(string(out))
}

// matchLocalRoute 解析 `ip route show table 100` 的输出，判断是否含本地路由。
func matchLocalRoute(out string) bool {
	for _, line := range strings.Split(out, "\n") {
		if strings.HasPrefix(strings.TrimSpace(line), "local ") {
			return true
		}
	}
	return false
}

// hasNftTable 检测 nftables 表是否已存在。
//
// `nft list table` 在表不存在时返回非零码；无权限时同样返回非零码，
// 两种情况都会被视为「无残留」，此时删除也必然失败，跳过是正确行为。
func hasNftTable() bool {
	// nft 会把报错写到 stderr，这里丢弃以免污染日志
	cmd := exec.Command("nft", "list", "table", "ip", tproxyNftTable)
	cmd.Stderr = &bytes.Buffer{}
	return cmd.Run() == nil
}

// tunnelIfacePrefixes 隧道/虚拟网络设备名前缀：这些设备自身的出站流量
// 不得被 TProxy mark 劫持，否则穿透链路会套进代理形成自锁断链。
// tun*    easytier / OpenVPN / 各类 VPN（本机现网：tun0）
// wg*     WireGuard
// Meta*   mihomo 自身 tun 设备（tun 模式开启时防止 output 被 mark 自锁）
// ipvl*   fnOS 虚拟网口（依附物理网卡，本机：ipvl0@enp2s0）
var tunnelIfacePrefixes = []string{"tun", "wg", "Meta", "ipvl"}

// isTunnelInterface 判断设备名是否属于隧道/虚拟设备。
// lo 与物理网卡（enp*/eth*/wlp*）和网桥（br-*/docker0）不在此列。
func isTunnelInterface(name string) bool {
	for _, p := range tunnelIfacePrefixes {
		if strings.HasPrefix(name, p) {
			return true
		}
	}
	return false
}

// listTunnelInterfaces 返回当前系统中实际存在的隧道设备名列表
// （解析 `ip -o link`，输出里设备名可能带 @父设备 后缀，取 @ 前部分）。
// 命令失败时返回空列表——bypass 规则缺失只是退化回原行为，不应阻断
// TProxy 主流程启用。
func listTunnelInterfaces() []string {
	out, err := exec.Command("ip", "-o", "link", "show").Output()
	if err != nil {
		log.Printf("[TProxy] 列举网络设备失败（跳过隧道设备 bypass）: %v", err)
		return nil
	}
	return parseTunnelInterfaces(string(out))
}

// parseTunnelInterfaces 纯函数解析 `ip -o link` 输出，便于单测。
// 行格式：`12: tun0@enp2s0: <BROADCAST,...> mtu 1500 ...`
func parseTunnelInterfaces(out string) []string {
	var names []string
	seen := map[string]bool{}
	for _, line := range strings.Split(out, "\n") {
		// 设备名在第二个冒号段起始处
		idx := strings.Index(line, ": ")
		if idx < 0 {
			continue
		}
		rest := line[idx+2:]
		end := strings.Index(rest, ":")
		if end <= 0 {
			continue
		}
		name := rest[:end]
		if at := strings.Index(name, "@"); at >= 0 {
			name = name[:at]
		}
		if isTunnelInterface(name) && !seen[name] {
			seen[name] = true
			names = append(names, name)
		}
	}
	return names
}

// tproxyNftTable 本模块使用的 nftables 表名。
const tproxyNftTable = "fluxor_tproxy"

// parseTproxyException 解析单条规则，返回 (ruleType, value, proto, port)
// 支持：
//   - IP/CIDR: 192.168.1.0/24
//   - 单个IP: 192.168.1.1
//   - 端口(所有协议): 53
//   - 协议:端口: tcp:80 或 udp:443
func parseTproxyException(rule string) (typ string, ipNet *net.IPNet, proto string, port int, err error) {
	rule = strings.TrimSpace(rule)
	if rule == "" {
		return "", nil, "", 0, fmt.Errorf("空规则")
	}
	// 尝试 IP/CIDR
	if _, ipNet, err := net.ParseCIDR(rule); err == nil {
		return "ip", ipNet, "", 0, nil
	}
	// 尝试单个 IP
	if ip := net.ParseIP(rule); ip != nil {
		_, ipNet, _ := net.ParseCIDR(rule + "/32")
		return "ip", ipNet, "", 0, nil
	}
	// 尝试 协议:端口
	if strings.Contains(rule, ":") {
		parts := strings.SplitN(rule, ":", 2)
		proto := strings.ToLower(parts[0])
		if proto != "tcp" && proto != "udp" {
			return "", nil, "", 0, fmt.Errorf("协议仅支持 tcp/udp")
		}
		p, err := strconv.Atoi(parts[1])
		if err != nil || p < 1 || p > 65535 {
			return "", nil, "", 0, fmt.Errorf("端口无效")
		}
		return "port", nil, proto, p, nil
	}
	// 尝试纯数字端口
	if p, err := strconv.Atoi(rule); err == nil && p > 0 && p <= 65535 {
		return "port", nil, "", p, nil
	}
	return "", nil, "", 0, fmt.Errorf("不支持的格式")
}

// EnableTProxyRules 配置策略路由与 nftables 规则（增强版）
// 支持目的例外、源例外以及本机出站流量控制
func EnableTProxyRules(port int) error {
	if port <= 0 {
		return nil
	}
	// TUN 与 TProxy 互斥后端兜底：两套都做出口流量重定向，叠加会让本机流量
	// 进入环回、整站打不开。前端 Config.vue 已做互斥但只拦 UI 路径，
	// cron / 手改配置 / 第三方直连都会绕过，故在真正落 nft 规则前再校验一次。
	if IsTUNEnabled() {
		return fmt.Errorf("TUN 与 TProxy 不可同时启用（TUN 已通过配置启用），请先关闭 TUN 再开启 TProxy")
	}

	// 辅助函数：执行命令并忽略错误（保留原行为）
	runCmd := func(name string, args ...string) {
		cmd := exec.Command(name, args...)
		if err := cmd.Run(); err != nil {
			log.Printf("[TProxy] 命令执行失败: %s %v, 错误: %v", name, args, err)
		}
	}

	// 1. 策略路由
	runCmd("ip", "rule", "add", "fwmark", "1", "table", "100")
	runCmd("ip", "route", "add", "local", "0.0.0.0/0", "dev", "lo", "table", "100")

	// 2. 关闭反向路径过滤
	runCmd("sysctl", "-w", "net.ipv4.conf.all.rp_filter=0")
	runCmd("sysctl", "-w", "net.ipv4.conf.default.rp_filter=0")
	runCmd("sysctl", "-w", "net.ipv4.conf.lo.rp_filter=0")

	// 3. 创建 nftables 表
	runCmd("nft", "add", "table", "ip", "fluxor_tproxy")

	// 4. 绕过私有网段
	runCmd("nft", "add", "set", "ip", "fluxor_tproxy", "private_ips", "{ type ipv4_addr; flags interval; }")
	bypassIPs := []string{"10.0.0.0/8", "127.0.0.0/8", "169.254.0.0/16", "172.16.0.0/12", "192.168.0.0/16", "224.0.0.0/4", "240.0.0.0/4"}
	for _, ip := range bypassIPs {
		runCmd("nft", "add", "element", "ip", "fluxor_tproxy", "private_ips", "{", ip, "}")
	}

	// 5. 创建链
	runCmd("nft", "add", "chain", "ip", "fluxor_tproxy", "prerouting", "{ type filter hook prerouting priority mangle; policy accept; }")
	runCmd("nft", "add", "chain", "ip", "fluxor_tproxy", "output", "{ type route hook output priority mangle; policy accept; }")
	runCmd("nft", "add", "chain", "ip", "fluxor_tproxy", "dstnat", "{ type nat hook prerouting priority -100; policy accept; }")
	runCmd("nft", "add", "chain", "ip", "fluxor_tproxy", "nat_output", "{ type nat hook output priority -100; policy accept; }")

	// 5a. 绕过隧道设备出站（防失联加固）
	// easytier(tun0/Meta*)、WireGuard(wg*)、fnOS 虚拟口(ipvl0) 等虚拟设备
	// 自身的出站流量一旦被 mark 劫持进 mihomo，穿透链路就会套在代理里自锁。
	// ip rule 不支持设备名条件，这里在 output 链按当前实际存在的隧道设备
	// 逐个 return（放在所有劫持规则之前）；fluxor 每次启动/开启 TProxy 都会
	// 重装，天然跟随设备变化。隧道 peer 的公网 IP 例外仍由用户在
	// tproxy_dst_exceptions 里自行配置。
	for _, dev := range listTunnelInterfaces() {
		runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "output", "oifname", dev, "return")
	}

	// 6. 私有IP绕过
	runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "prerouting", "fib", "daddr", "type", "local", "return")
	runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "prerouting", "ip", "daddr", "@private_ips", "return")
	runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "output", "ip", "daddr", "@private_ips", "return")

	// 7. 加载例外列表
	dstExceptions := LoadTproxyDstExceptions()
	srcExceptions := LoadTproxySrcExceptions()

	// 7a. 目的例外
	for _, rule := range dstExceptions {
		rule = stripComment(rule)
		if rule == "" {
			continue
		}
		typ, ipNet, proto, portVal, err := parseTproxyException(rule)
		if err != nil {
			log.Printf("[TProxy] 跳过无效目的例外规则 %q: %v", rule, err)
			continue
		}

		if typ == "ip" {
			cidr := ipNet.String()
			// TProxy 劫持
			runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "prerouting", "ip", "daddr", cidr, "return")
			runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "output", "ip", "daddr", cidr, "return")
			// DNS 重定向
			runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "dstnat", "ip", "daddr", cidr, "return")
			if tproxyProxyLocal {
				runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "nat_output", "ip", "daddr", cidr, "return")
			}
		} else if typ == "port" {
			// 将集合作为一个完整的字符串参数
			protoExpr := "{tcp, udp}"
			if proto != "" {
				protoExpr = "{" + proto + "}"
			}
			// TProxy 劫持
			runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "prerouting", "meta", "l4proto", protoExpr, "th", "dport", strconv.Itoa(portVal), "return")
			runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "output", "meta", "l4proto", protoExpr, "th", "dport", strconv.Itoa(portVal), "return")
			// DNS 重定向
			runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "dstnat", "meta", "l4proto", protoExpr, "th", "dport", strconv.Itoa(portVal), "return")
			if tproxyProxyLocal {
				runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "nat_output", "meta", "l4proto", protoExpr, "th", "dport", strconv.Itoa(portVal), "return")
			}
		}
	}

	// 7b. 源例外
	for _, rule := range srcExceptions {
		rule = stripComment(rule)
		if rule == "" {
			continue
		}
		if _, ipNet, err := net.ParseCIDR(rule); err == nil {
			cidr := ipNet.String()
			runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "prerouting", "ip", "saddr", cidr, "return")
			runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "dstnat", "ip", "saddr", cidr, "return")
		} else if ip := net.ParseIP(rule); ip != nil {
			cidr := ip.String() + "/32"
			runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "prerouting", "ip", "saddr", cidr, "return")
			runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "dstnat", "ip", "saddr", cidr, "return")
		} else {
			log.Printf("[TProxy] 源例外仅支持 IP/CIDR，忽略无效规则: %s", rule)
		}
	}

	// 8. TProxy 劫持规则
	// 使用集合字符串 "{tcp,udp}"
	runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "prerouting", "meta", "l4proto", "{tcp,udp}", "tproxy", "to", fmt.Sprintf(":%d", port), "meta", "mark", "set", "1", "accept")
	runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "output", "meta", "mark", "0xff", "return")
	runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "output", "socket", "mark", "0xff", "return")
	if tproxyProxyLocal {
		runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "output", "meta", "l4proto", "{tcp,udp}", "meta", "mark", "set", "1", "accept")
	}

	// 9. DNS 重定向（目标端口由 tproxy_dns_redirect_port 配置，默认 1053）
	dnsPort := fmt.Sprintf(":%d", GetDNSRedirectPort())
	runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "dstnat", "udp", "dport", "53", "redirect", "to", dnsPort)
	runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "dstnat", "tcp", "dport", "53", "redirect", "to", dnsPort)
	if tproxyProxyLocal {
		runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "nat_output", "meta", "mark", "0xff", "return")
		runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "nat_output", "socket", "mark", "0xff", "return")
		runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "nat_output", "udp", "dport", "53", "redirect", "to", dnsPort)
		runCmd("nft", "add", "rule", "ip", "fluxor_tproxy", "nat_output", "tcp", "dport", "53", "redirect", "to", dnsPort)
	}

	// 结果校验：上面各条命令的失败此前只写日志，导致面板在规则实际未生效时
	// 仍显示「已启用」。这里以表是否真正建成作为判定依据——若 nft 不可用或
	// 权限不足，表不存在，据此如实返回失败，由调用方决定是否回滚开关状态。
	if !hasNftTable() {
		return fmt.Errorf("nftables 表 %s 未建成，规则未生效（请确认 nft 可用且权限充足）", tproxyNftTable)
	}

	log.Printf("[TProxy] 规则应用成功（含目的/源例外及本机代理开关）")
	return nil
}

// DisableTProxyRules 清理本模块写入的 nftables 表与策略路由。
//
// 关键点在于「各项独立探测、存在才删」：
//
// EnableTProxyRules 先写策略路由（ip rule / ip route）再建 nft 表。若建表失败
// （nft 未安装、权限不足），就会出现「有策略路由、无 nft 表」的中间状态。
// 此前实现把 ip 规则的清理放在「nft 表存在」的判定之后，导致这种情形下
// 策略路由永远清不掉——流量被导入空的 table 100 → 持续断网。
//
// 因此每项资源各自探测：有残留才执行删除，既不会漏删（不再被 nft 表的
// 存在性绑架），也不会对不存在的对象执行 del 而徒增错误。
//
// 幂等，可重复调用。
func DisableTProxyRules() {
	removed := make([]string, 0, 3)

	if hasNftTable() {
		if err := exec.Command("nft", "delete", "table", "ip", tproxyNftTable).Run(); err != nil {
			log.Printf("[TProxy] 删除 nftables 表失败: %v", err)
		} else {
			removed = append(removed, "nft 表")
		}
	}

	if hasLocalRoute() {
		if err := exec.Command("ip", "route", "del", "local", "0.0.0.0/0", "dev", "lo", "table", tproxyTableID).Run(); err != nil {
			log.Printf("[TProxy] 删除策略路由失败: %v", err)
		} else {
			removed = append(removed, "策略路由")
		}
	}

	if hasFwmarkRule() {
		if err := exec.Command("ip", "rule", "del", "fwmark", tproxyFwmark, "table", tproxyTableID).Run(); err != nil {
			log.Printf("[TProxy] 删除路由规则失败: %v", err)
		} else {
			removed = append(removed, "路由规则")
		}
	}

	if len(removed) == 0 {
		log.Printf("[TProxy] 未发现残留规则，无需清理")
		return
	}
	log.Printf("[TProxy] 已清理: %s", strings.Join(removed, "、"))
}

// stripComment 去除行尾 # 注释，并 trim 空格，返回纯净的规则部分
func stripComment(line string) string {
	line = strings.TrimSpace(line)
	if idx := strings.Index(line, "#"); idx >= 0 {
		line = strings.TrimSpace(line[:idx])
	}
	return line
}
