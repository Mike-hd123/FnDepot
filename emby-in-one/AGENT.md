# emby-in-one — 子项目 AGENT 须知

> 通用规矩见根 [AGENT.md](../AGENT.md)。本文件讲 emby-in-one 特有的架构、定制清单与血泪坑。

## 定位
Emby 多账号聚合反向代理（基于 [ArizeSky/Emby-In-One](https://github.com/ArizeSky/Emby-In-One) GPL-3.0，v1.4.4-rc1）。解决上游 Emby Boost CDN 签名 URL 900 秒过期导致 TV 端拖拽 403。本机现役服务端口 **18096**，桌面入口 `/app/emby/admin/`（socket 模式）。

## 架构（v1.4.4-3 起 = Go 后端 + Python socket sidecar 双进程）
```
fnOS 桌面图标 → trim 网关 → app.sock（sidecar）→ 127.0.0.1:18096（Go 后端）
TV/客户端     → 直连 TCP 18096（不走 sidecar）
```
- `app/gateway/emby-gateway.py`：Unix socket 反代 sidecar，剥 `/app/emby` 前缀、根路径 302 → `/app/emby/admin/`
- **管理员自动重登（v1.4.4-4）**：后端 admin token 是纯内存态，服务重启即废。sidecar 拦 `/admin/api/*` 的 401 → 查 stale_map 旧→新映射 → 无则用 `EIO_ADMIN_USER/PASS`（默认 admin/admin）调 `POST /Users/AuthenticateByName` 拿新 token 重放（≤2 发、30s 失败退避、登录互斥锁）。映射落盘 `GATEWAY_STATE_FILE`（`@appdata/gateway_state.json`），sidecar 重启不丢。非 `/admin/api` 路径（TV 播放/客户端登录）零干预。
- Go 二进制不在 git：build 前从 `/vol2/1000/workspace/emby-in-one/repo/`（fork，含定制）静态编译 `CGO_ENABLED=1 go build -tags timetzdata -trimpath -ldflags '-s -w -extldflags "-static"' -o app/emby-in-one ./cmd/embyinone`，产物 10MB，`app/emby-in-one` 被 .gitignore 屏蔽
- `cmd/main`：status/start/stop 三连；start = Go 后端 + sidecar 双拉起；stop 用 pgrep 兜底（as_user 子 shell `$!` 错位坑）；status = main 脚本参数 + pgrep 双进程校活

## 本地定制清单（上游同步时逐条核对存活！）
1. **`src/internal/backend/upstream.go` validateAPIKey 补丁**（diff 快照：`docs/validateApiKey.patch`）：Emby Boost 不支持 `/Users/Me` → 回退 `/Users` 列表匹配；带用户级 `X-Emby-Token` 转发（进度隔离）；`IsApiKeyValid` 缓存失效修复
2. **打包层 sidecar + socket 桌面入口**（`app/gateway/`、`ui/config` gatewaySocket=app.sock、manifest `micro_app=true`）
3. **admin/admin 明文默认密码**（用户拍板：家庭内网 + 网关自动重登依赖账密，不上随机码）
4. 静态编译 Go（免 glibc 依赖）；预配上游 entertang（redirect 模式）

## 血泪坑（别重蹈）
- **manifest 缺 `micro_app=true` → 桌面图标点击不通**（fnpack build 会重写 manifest 丢字段！打包后必反解核验 `grep -c micro_app manifest`）
- socket sidecar 应用：`ui/config` 需 `gatewaySocket`+`gatewayPrefix`，二者同缺=纯 iframe 404
- `POST /admin/api/login` 不存在（404）——admin UI 实际调 `POST /Users/AuthenticateByName`（body `{"Username","Pw"}`，响应 `AccessToken`）；admin.js 按**请求头** `X-Emby-Client` 区分客户端流量（无此头才注入 token）
- trim_sac 桌面入口按磁盘 manifest 刷新（重启应用触发），手改 DB 会被下次刷新覆盖 → **改注册信息必双改（磁盘 manifest + DB）**
- 后端 `/admin/api/*` 只认 `X-Emby-Token` **header**（query `api_key` 401）；后台 API 重启后旧 token 全废（内存 authCodes）
- 代理配置姿势：先建代理池再在"上级服务器"绑 proxyId，直接填 URL 无效
- 图标撞车：worker 曾把混元记忆图标抄进来（md5 实锤）——新应用图标必须自绘，`generate_icons.py`

## 版本口径
上游 1.4.4-rc1 → `1.4.4-N`（本地修订从 1 起）：
- 1.4.4-1 首发（装机失败：cmd/main 缺 cd）
- 1.4.4-2 桌面入口/密码/图标/显示名修复
- 1.4.4-3 socket 网关 sidecar（manifest 漏 micro_app，装机侧双写补救）
- 1.4.4-4 micro_app 固化 + **管理员自动重登**（e2e 5 场景全过，用户实测通过）

## 工具链
- 源码 fork：`/vol2/1000/workspace/emby-in-one/repo/`（git@ArizeSky，定制未 commit——同步上游前先 commit 或 stash）
- Go：`/vol2/1000/workspace/tools/go1.27/bin`
- 打包：`./build.sh`（fnpack 1.2.4），产物 `/vol2/1000/download/emby-in-one-<ver>-x86.fpk`
- e2e：`tests/gw_autologin_e2e.py`（假后端+真 sidecar，A登录/B重启救回/C-PUT重试/D防风暴/E非admin不干预）
- 排障报告：`docs/sidecar-autologin-20260928.md`
