# octopus — Agent 须知

> 项目说明见 [README.md](./README.md)。本文件只讲 agent 该做什么 / 红线 / 坑。

## 定位
上游 [bestruirui/octopus](https://github.com/bestruirui/octopus) 的 fnOS 发行：**LLM API 聚合网关**（Go 单二进制 + 内嵌前端 + SQLite）。当前发布 **0.13.5-1**，端口 **8081**——Hermes/本 agent 的模型网关，挂了=agent 失智，最高优先级服务。

## 架构
- 二进制=官方 release 原样（前端已嵌入，不改源码）；本地价值在 **fnOS 打包层** `src/fnos/`（cmd/manifest/config/gateway）
- 数据：`/vol1/@appdata/octopus/data.db`（TRIM_PKG_VAR 注入，卸载重装不丢）
- 渠道 key 在 DB `channel_keys`；改协议位/分组后**必须重启**才生效（protocols 位图缺 Chat 位(12)会导致 chat 静默跳过 grant，补 14 后重启）

## gateway sidecar（本地核心补丁，v8，合并上游时勿丢）
飞牛统一网关 socket 自注册：sidecar 监听 `APPDEST/app.sock` → 反代 TCP 8081，手机端 `/app/octopus` 走它。演进：裸 TCP 盲转发→剥前缀 HTTP 反代（修 404）→ SSE 流式透传 + POST Content-Length 透传 + JS chunk 同版本参数注入（保单模块图单实例，ThemeProvider 不断裂）。版本号重置规矩：上游 minor 升级 → 本地补丁号回 `-1`。

## 坑
- 装/升级会瞬断模型出口——agent 自己掉线，操作前知会用户
- 端口 8081 避开云微 8080；数据备份=单文件 data.db
- 运维细节 → Hermes 技能 `octopus-gateway-ops`；生图渠道实测法（/v1/models 不列图片模型，须直连 images 端点）在其 references/image-gen-tokenrhythm.md

## 红线
同根 [AGENT.md](../AGENT.md)。
