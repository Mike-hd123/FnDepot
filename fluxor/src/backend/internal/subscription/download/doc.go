package download

// Package download 负责把远程订阅拉取为本地节点文件。
//
// 采用「直连优先、临时内核回退」策略：
//  1. direct.go  直接 HTTP 下载，仅接受原样即为 Clash 明文 YAML 的响应，
//     并解析 subscription-userinfo 头
//  2. 直连内容不是 Clash YAML（Base64 编码的 URI 列表、裸 URI 列表、age 加密等）
//     时回退到临时内核实例完成下载——格式识别与解码统一交由内核，Fluxor 不自行实现
//
// 依赖方向（与 AGENTS.md 的依赖图一致，可用
// `go list -f '{{.Imports}}' ./internal/subscription/download` 复核）：
//
//	config, configcheck, core
//
// 注意本包并【不是】叶子包：download.go 会调用 core.DownloadWithTempCore 走临时
// 内核回退。为打破 core <-> subscription 的循环依赖，临时内核的下载实现被下沉到
// core 包（core/tmpcore.go），core 只接收 config.Subscription 数据结构，不反向
// 引用本包，因此单向链 core ← subscription/download 不构成环。
