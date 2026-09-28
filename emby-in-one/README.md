# Emby 聚合代理 (emby-in-one) — FnDepot 应用

## 定位
Emby 多账号聚合反向代理，解决上游 Emby Boost CDN 签名 URL 900 秒过期导致拖拽 403 问题。
基于 [ArizeSky/Emby-In-One](https://github.com/ArizeSky/Emby-In-One) V1.4.4-rc1（GPL-3.0，见 [LICENSE](./LICENSE)，fork 源码含定制在 [`src/`](./src/)）。

## 架构
- **形态**: 原生 fnOS 应用 = Go 单二进制后端（静态编译 CGO+SQLite）+ Python socket 网关 sidecar
- **后端端口**: TCP 18096（TV/Emby 客户端直连此端口）
- **桌面入口**: 飞牛桌面图标 → fnOS 网关 Unix socket（`app.sock`）→ sidecar 反代 → 18096，路径 `/app/emby/admin/`
- **数据目录**: `@appdata/emby-in-one/`（config.yaml + mappings.db + tokens.json + gateway_state.json + 日志）
- **运行用户**: user（非 root）

## 管理员自动重登（v1.4.4-4）
后端 admin token 是纯内存态，**服务重启即失效**，以前每次进后台都要重新登录。现 sidecar 网关内置自动续命：
- `/admin/api` 请求 401 → 自动用 `admin/admin` 重新登录拿新 token → 重放原请求，浏览器无感
- 旧→新 token 映射落盘 `gateway_state.json`，sidecar 重启不丢
- TV/客户端流量（`/emby/*`、播放）**零干预**
- 账密可通过 env `EIO_ADMIN_USER/EIO_ADMIN_PASS` 覆盖（cmd/main 默认注入 admin/admin）

## 本地定制清单
1. `src/internal/backend/upstream.go` — `validateAPIKey` 补丁：上游 Emby Boost 不支持 `/Users/Me`，回退 `/Users` 列表匹配 + 用户级 token 转发（进度隔离）+ `IsApiKeyValid` 缓存失效修复（diff 快照 `docs/validateApiKey.patch`）
2. socket 网关 sidecar + 桌面入口（`app/gateway/emby-gateway.py`，manifest `micro_app=true`）
3. 静态编译：`CGO_ENABLED=1 -extldflags '-static'`（内置 SQLite，无外部 GLIBC 依赖）
4. 预配上游 entertang 服务器，redirect 播放模式

## 端口表
| 端口 | 用途 |
|------|------|
| 18096 | EIO 主服务（Emby API 兼容 + 管理面板 /admin/）|

## 默认账号
后台 `http://<NAS>:18096/admin/`：用户名 `admin` / 密码 `admin`（家庭内网口径，可在 config.yaml 修改）。

## 构建
```bash
# Go 源码在 /vol2/1000/workspace/emby-in-one/repo/（fork 含定制；静态编译产物 app/emby-in-one 不入库）
cd /vol2/1000/workspace/FnDepot/emby-in-one
./build.sh
# 产物: /vol2/1000/download/emby-in-one-1.4.4-4-x86.fpk
# ⚠️ 打包后必反解核验 manifest 的 micro_app=true（fnpack 重写坑）
```

## 测试
```bash
python3 tests/gw_autologin_e2e.py   # 5 场景：登录捕获/重启救回/PUT重试/防风暴/非admin隔离
```

## 版本历史
- **1.4.4-4**（2026-09-28）: sidecar 管理员自动重登（401 无感续命 + stale_map 落盘）；micro_app 固化进打包源；e2e 5 场景全过，用户实测通过。
- **1.4.4-3**（2026-09-28）: socket 网关 sidecar + 桌面入口切 socket 模式；cmd/main 双进程起停 + pgrep 兜底。
- **1.4.4-2**（2026-09-27）: 桌面入口修复、明文默认密码 admin、显示名 Emby、自绘图标。
- **1.4.4-1**（2026-09-26）: 首次打包。基于 V1.4.4-rc1 + validateAPIKey 补丁 + 静态编译。
