# octopus — Agent 须知

> 项目说明见 [README.md](./README.md)。本文件只讲 agent 该做什么 / 红线 / 坑。

## 定位
上游 [bestruirui/octopus](https://github.com/bestruirui/octopus) 的 fnOS 发行：**LLM API 聚合网关**（Go 单二进制 + 内嵌前端 + SQLite）。当前发布 **0.13.12-1**（2026-10-14），端口 **8081**——Hermes/本 agent 的模型网关，挂了=agent 失智，最高优先级服务。

## 架构
- 二进制=官方 release 原样（前端已嵌入，不改源码）；本地价值在 **fnOS 打包层** `src/fnos/`（cmd/manifest/config/gateway）
- 数据：`/vol1/@appdata/octopus/data.db`（TRIM_PKG_VAR 注入，卸载重装不丢）；监听开关 `listen.conf` 同目录
- 渠道 key 在 DB `channel_keys`；改协议位/分组后**必须重启**才生效（protocols 位图缺 Chat 位(12)会导致 chat 静默跳过 grant，补 14 后重启）

## 监听地址开关（0.13.12-1 新增）
默认 **127.0.0.1**（只服务本机 fnOS 网关 sidecar + DSH 本机模型出口）。要局域网直连就把 `/vol1/@appdata/octopus/listen.conf` 改成 `OCTOPUS_LISTEN_HOST=0.0.0.0` 再重启应用。
- 直接复用**上游原生键** `OCTOPUS_SERVER_HOST`（viper AutomaticEnv + prefix octopus + `.`→`_`），**别再发明第二层配置**
- 上游默认是 0.0.0.0，本包默认收紧回环＝本地安全策略，不是上游行为
- 环境变量 `OCTOPUS_LISTEN_HOST` 可临时覆盖配置文件
- 白名单只认 `127.0.0.1`/`0.0.0.0`，非法值告警回落默认
- 历史背景：fnOS 网关注入 host=127.0.0.1 曾导致只听回环，只好用 socat 桥 8082 绕路；此开关是正解

## gateway sidecar（本地核心补丁，合并上游时勿丢）
飞牛统一网关 socket 自注册：sidecar 监听 `APPDEST/app.sock` → 反代 TCP 8081，手机端 `/app/octopus` 走它。演进：裸 TCP 盲转发→剥前缀 HTTP 反代（修 404）→ SSE 流式透传 + POST Content-Length 透传 + JS chunk 同版本参数注入（保单模块图单实例，ThemeProvider 不断裂）。版本号重置规矩：上游 minor 升级 → 本地补丁号回 `-1`（勿一路自增，2026-10-08 曾因连用 -2~-6 掩盖版本号语义）。
**打包必须走 fnpack**：解上一版 fpk 作骨架 → 换二进制与 sidecar → 同步 `fnos/cmd`+`ui`+`wizard` 整层 → 删 `app/config` → 改 manifest（删 checksum 行）→ `fnpack build -d .`。自写 tar 脚本会漏 ui/images、wizard/install、LICENSE 等，产物与既定包不一致。

## 坑
- ⚠️ **「存在性自检」挡不住旧内容**（2026-10-14 血泪）：`build-fpk.sh` 原先不复制 `fnos/cmd/`，骨架里的旧 `cmd/main` 被静默带过去，**新功能根本没打进包而构建全绿**。已修（新增 3b 步同步整层 + 逐文件 `cmp` + 5/5 内容级自检 + 开关回归护栏）。**改完 cmd/main 一定要 `diff` 包内与源文件，别只 `grep` 存在性。**
- **v0.13.10 起 sidecar 不能再手工改写 API 前缀**：上游 bdc9948 让前端自带子路径解析（`fetch(new URL("\."+path, document.baseURI))`），sidecar 再改写会叠加成 `/app/octopus/app/octopus/api/v1/...` 导致登录 404。故 `GATEWAY_API_REWRITE` 默认 0；回退到 ≤v0.13.9 才设 1。
- 装/升级会瞬断模型出口——agent 自己掉线，操作前知会用户
- 端口 8081 避开云微 8080；数据备份=单文件 data.db
- 运维细节 → Hermes 技能 `octopus-gateway-ops`；生图渠道实测法（/v1/models 不列图片模型，须直连 images 端点）在其 references/image-gen-tokenrhythm.md

## 红线
同根 [AGENT.md](../AGENT.md)。
