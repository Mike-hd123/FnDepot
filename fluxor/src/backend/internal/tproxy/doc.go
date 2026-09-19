package tproxy

// Package tproxy 管理 TProxy 透明代理的防火墙（nftables）与策略路由规则。
//
// 安全约束（务必遵守）：
//   - 严禁拼接 shell 字符串并用 sh -c 执行；必须使用 exec.Command 原生参数切片，
//     以杜绝用户表单（例外 IP/端口）带来的命令注入。
//   - 面板退出时必须调用 DisableTProxyRules 清退全部规则，避免断网残留。
//   - 冷启动必须调用 ResetOnStartup：nft 规则不跨重启存活而开关状态是持久化的，
//     归零开关并清残留，避免「面板显示关闭、流量仍被劫持」。
//   - 清理时各项资源（nft 表 / fwmark 规则 / local 路由）各自探测、存在才删，
//     不得把策略路由的清理绑在「nft 表存在」的判定之后（详见 rules.go）。
//
// 文件划分：
//   - state.go   启用状态、例外缓存与各读写锁
//   - store.go   开关状态 / 例外列表 / 本机代理开关在 fluxor.json 中的持久化，
//                以及冷启动收敛 ResetOnStartup
//   - rules.go   规则解析与 nftables 规则的启用、探测式清理
//   - handlers.go /config/tproxy* HTTP 接口
