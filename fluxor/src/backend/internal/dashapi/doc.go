package dashapi

// Package dashapi 把内核的 HTTP API 反向代理给前端。
//
// 所有请求都经由 core.CoreRequest 发往内核 Unix Socket，由后者统一附加
// Bearer 认证并处理 Context 释放。本包只做路径解析、方法校验与响应透传。
//
// 文件划分：
//   - dashboard.go   版本等只读仪表盘接口
//   - configs.go     内核配置的读写、重载、GEO/缓存维护、DNS 查询
//   - proxies.go     代理组列表、单节点测速、策略组批量测速
//   - providers.go   订阅代理信息（含流量/有效期）
//   - rules.go       规则与规则提供商
//   - connections.go 连接断开
//   - upgrade.go     内核升级
//   - pathparam.go   拼入内核路径的片段校验（防路径穿越）
//
// 实时流量 / 内存 / 连接数据不走本包：前端经 WebSocket（/traffic、/memory、
// /connections）由 wsproxy 双向桥接，不再提供对应的 HTTP 拉取接口。
