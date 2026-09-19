package delaytest

// Package delaytest 通过代理测量到目标站点的连通延迟。
//
// 使用 HEAD 请求且禁用重定向，仅需首包响应即可判定连通性；请求附带浏览器 UA
// 与 Connection: close，避免被 WAF 拦截或等待响应体结束导致误报超时。
