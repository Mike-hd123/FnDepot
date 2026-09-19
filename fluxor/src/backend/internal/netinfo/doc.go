package netinfo

// Package netinfo 提供网络信息查询能力。
//
// 文件划分：
//   - interfaces.go 系统网卡枚举与本地出口 IP 回退
//   - iplookup.go   经代理或直连查询公网 IP（兼容 JSON 与纯文本响应）；
//                   newLookupClient 为一次性查询构造禁用 keep-alive 的客户端
//   - geo.go        IP 归属地（国家/地区/ISP）查询，带 10 分钟缓存且条目数有上限
//   - proxyport.go  从内核配置读取实际生效的代理端口
//   - addr.go       TCP 监听地址格式校验
//   - handlers.go   /ipinfo/* 与 /interfaces HTTP 接口
//
// 外部查询（iplookup / geo）均属低频一次性操作，客户端统一禁用 keep-alive：
// 响应读完即断开，避免连接长期停留在 ESTABLISHED（默认 IdleConnTimeout 长达
// 90 秒）而在面板「连接」页留下常驻条目。
