package core

// Package core 管理 Mihomo 内核进程的生命周期。
//
// 职责包括：
//   - client.go   与内核 Unix Socket 通信的 HTTP 客户端（含 cancelableReadCloser，
//     确保大 JSON 响应读取完毕后再释放 Context，避免流被提前截断）
//   - lifecycle.go 启动 / 停止 / 热重载（重载后异步同步 nftables TProxy 规则）
//   - logger.go    内核操作日志文件记录器
//   - tmpcore.go   启动临时内核实例下载订阅节点文件并回读元数据
//   - events.go    内核运行状态的变更广播（SSE /core/events）
//   - handlers.go  对外的 /core/* HTTP 接口
//
// 依赖方向：config / configgen / tproxy。注意本包不依赖 subscription——
// 需要生成配置时只依赖纯渲染层 configgen。
//
// 内核状态有两条变更来源，都必须广播（见 events.go）：本进程发起启停操作、
// 以及内核自行退出（崩溃/被外部 kill，由等待子进程的 goroutine 捕获）。
