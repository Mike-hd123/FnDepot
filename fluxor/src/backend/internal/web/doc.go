package web

// Package web 负责前端单页应用的入口渲染。
//
//   - index.go  index.html 模板渲染（注入 BaseHref 与 RawBase，支持自定义前缀）
//   - whoami.go 从 X-Trim-Username 请求头读取当前用户
