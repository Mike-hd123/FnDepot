package appupdate

// Package appupdate 处理 Fluxor 自身与 Mihomo 内核的版本检查及自更新。
//
// 文件划分：
//   - github.go      GitHub Release / Tag 查询与语义化版本比较
//   - cache.go       版本信息内存缓存（TTL 10 分钟）
//   - coreversion.go 内核本地版本读取（经 Unix Socket）与远程版本比对
//   - selfupdate.go  Fluxor 自更新：多加速源+代理回退下载、备份、替换并重启
//   - handler.go     /check-update HTTP 接口
