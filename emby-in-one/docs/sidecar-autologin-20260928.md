# emby-in-one fpk v1.4.4-3 改造报告

**日期**: 2026-09-28  
**任务**: t_e0bd6bed  
**产物**: `/vol2/1000/download/emby-in-one-1.4.4-3-x86.fpk`  
**大小**: 4.3MB  
**SHA256**: `568526ba222410b88b96951ad7f94d60c7f06c919fd980c3cf3d9c1504264b07`  
**版本**: 1.4.4-3

---

## 改造点清单

### 1. 新增 `app/gateway/emby-gateway.py` — socket 网关 sidecar

照抄 hyatlas `gateway_proxy.py` 模式改造，核心机制：

- **Unix socket 反代**: `APPDEST/app.sock` → `127.0.0.1:18096`（emby-in-one Go 后端）
- **剥前缀**: fnOS 桌面走 `/app/emby/<path>` 前缀，sidecar 剥掉前缀后转发后端裸路径
- **根路径重定向**: `/app/emby/` → 302 → `/app/emby/admin/`（admin 后台是桌面入口，不是 Emby 客户端占位页）
- **JS 响应体绝对路径重写**（emby SPA 不支持 base URL，JS 里全是绝对路径）：

| 原始路径 | 重写后 | 用途 |
|---|---|---|
| `/admin/api/*` | `/app/emby/admin/api/*` | admin 后台 API |
| `/Users/*` | `/app/emby/Users/*` | Emby 认证 API (AuthenticateByName) |
| `/admin/` (非 api) | `/app/emby/admin/` | admin 其他引用 |
| `/libraries` | `/app/emby/libraries` | Emby 客户端 API |
| `/reconnect` | `/app/emby/reconnect` | Emby 客户端重连 |

- **HTML 响应体重写**: `/emby/` 占位页的 `href="/admin"` → `href="/app/emby/admin"`
- **Location 头补全**: 后端返回的绝对路径 Location 自动补 `/app/emby` 前缀
- **Origin 改写**: 强制 `http://127.0.0.1:18096`（CORS）
- **Accept-Encoding 剥离**: 强制后端返明文 + Content-Length（hyatlas 同款）
- **SSE 流式**: event-stream 走原始 socket 增量泵
- **Cache-Control**: 全局 `no-cache, max-age=0, must-revalidate`

### 2. 改造 `cmd/main` — 增 sidecar 起停 + 双活探活

- `start` 分支：先起 emby-in-one 二进制 → 再起 gateway sidecar（`GATEWAY_SOCK_PATH/GATEWAY_BACKEND_PORT/GATEWAY_PID_FILE` 环境变量注入）
- `stop` 分支：先停 sidecar（stop_gw）再停主进程（stop_app）
- `status` 分支：**main + gateway 双活才 exit 0**，防 sidecar 挂了但 main 活的「UI 停用」误判
- `is_running()` + `gw_running()` 均加 `pgrep` 兜底校准（防 `nohup ... &` 的 `$!` 过渡 shell PID 坑——hyatlas 同款教训）
- sidecar 启动后检查 `app.sock` socket 文件存在
- config.yaml 持久化：VAR_DIR 副本优先，fallback 到打包默认

### 3. 改造 `ui/config` — 切 socket 模式

旧（裸 port 直连）:
```json
{"port":"18096","url":"/admin/"}
```

新（socket 网关）:
```json
{
  "type": "iframe",
  "protocol": "",
  "gatewayPrefix": "/app/emby",
  "gatewaySocket": "app.sock",
  "url": "/app/emby/admin/",
  "allUsers": true
}
```

- **去掉 `port` 字段** — 不再裸 TCP 直连
- `gatewaySocket` = `app.sock`（相对于 @appcenter/emby-in-one/ 根目录解析）
- `gatewayPrefix` = `/app/emby`（fnOS 桌面统一前缀）
- `url` = `/app/emby/admin/`（桌面入口直指 admin 后台）

### 4. 版本号联动

- `manifest` version: 1.4.4-2 → **1.4.4-3**
- `build.sh` VERSION: 1.4.4-2 → **1.4.4-3**
- changelog 同步更新

### 5. build.sh 增强

- 新增 `app/gateway/emby-gateway.py` 存在性检查（FATAL if missing）
- 自检段新增：gateway sidecar 在 app.tgz 内层验证、ui/config socket 模式验证

---

## sidecar 路径重写规则及依据

### 实测 curl 证据（2026-09-28，对 127.0.0.1:18096 只读 GET）

```
=== Test 1: GET /app/emby/admin/ ===
HTTP 200

=== Test 2: HTML 资源引用（全相对路径）===
href="vendor/inter.css"          ← 相对，不需要重写
href="vendor/tailwind.css"       ← 相对
src="vendor/vue.global.prod.js"  ← 相对
src="admin.js"                   ← 相对

=== Test 3: vendor JS/CSS 经 socket 全 200 ===
vendor/vue.global.prod.js: HTTP 200
vendor/tailwind.css: HTTP 200
admin.js: HTTP 200

=== Test 4: admin.js API 路径全被重写 ===
'/app/emby/Users/AuthenticateByName'      ← 原 /Users/AuthenticateByName
'/app/emby/admin/api/client-info'         ← 原 /admin/api/client-info
'/app/emby/admin/api/logs'                 ← 原 /admin/api/logs
'/app/emby/admin/api/proxies'              ← 原 /admin/api/proxies
'/app/emby/admin/api/settings'             ← 原 /admin/api/settings
'/app/emby/admin/api/users'                ← 原 /admin/api/users
'/app/emby/libraries'                      ← 原 /libraries
'/app/emby/reconnect'                      ← 原 /reconnect

=== Test 5: /app/emby (无尾斜杠) → 301 ===
HTTP 301 Location: /app/emby/

=== Test 6: /app/emby/ 根 → 302 ===
HTTP 302 Location: /app/emby/admin/

=== Test 7: /emby/ 占位页 href 重写 ===
href="/app/emby/admin"  ← 原 href="/admin"
```

### 重写规则设计依据

1. **admin SPA 资源全用相对路径**（vendor/、admin.js）→ sidecar 不需要重写 HTML 里的 src/href（除 /emby/ 占位页的绝对链接）
2. **admin.js fetch 全用绝对路径** → sidecar 在 JS 响应体里做正则重写，按最长前缀优先匹配（`/admin/api/` 优先于 `/admin/`，避免误吞）
3. **emby-in-one 二进制不支持 base URL** → 不能靠后端配前缀，只能 sidecar 拦截响应体
4. **`/` 返回 Emby 客户端占位页**（不是 admin 后台）→ sidecar 根路径 302 重定向到 `/app/emby/admin/`

---

## 反解 fpk 逐项验收

| 检查项 | 结果 |
|---|---|
| fpk 格式（gzip+tar） | ✅ 合法 |
| manifest version | ✅ 1.4.4-3 |
| manifest display_name | ✅ Emby |
| ui/config 在 app.tgz 内层 | ✅ |
| ui/config 含 gatewaySocket=app.sock | ✅ |
| ui/config 无 port 字段 | ✅ |
| gateway/emby-gateway.py 在 app.tgz 内层 | ✅ |
| 无 __pycache__ | ✅ |
| config.yaml password: admin 明文 | ✅ |
| 图标 md5 不撞 hyatlas | ✅ (bcf6e9... vs 55148d...) |
| cmd/main 含 emby-gateway.py 引用 (×4) | ✅ |
| cmd/main 含 pgrep 兜底 (×3) | ✅ |
| cmd/main 含 gw_running (×5) | ✅ |
| cmd/main 含 stop_gw (×3) | ✅ |
| bash -n cmd/main 语法检查 | ✅ |
| python3 -m py_compile sidecar | ✅ |
| sidecar 实测 7 项全通过 | ✅ |

---

## 装机注意事项

### 1. `/var/apps/<app>/cmd/main` 升级不覆盖（fnOS 已知坑）

fnOS install-fpk 升级只刷 `@appcenter/`，`/var/apps/<app>/cmd/main` 是独立缓存。**升级后必须手动 cp 新 cmd/main 过去**：

```bash
sudo cp /vol1/@appcenter/emby-in-one/cmd/main /var/apps/emby-in-one/cmd/main
```

先备份：
```bash
sudo cp /var/apps/emby-in-one/cmd/main /var/apps/emby-in-one/cmd/main.bak_$(date +%Y%m%d)
```

### 2. config.yaml 升级不覆盖

fnOS 把 `app/config/` 当用户数据，install-fpk 升级保留旧文件。新包默认 config 不会生效，需手动 cp：

```bash
sudo cp /vol1/@appcenter/emby-in-one/config/config.yaml /vol1/@appdata/emby-in-one/config.yaml
```

### 3. 装后必须重启

```bash
sudo appcenter-cli stop emby-in-one
sudo appcenter-cli start emby-in-one
```

旧进程 env 里烤着旧版本字符串，不重启的话版本显示不变 + 新前端戳旧后端报错。

### 4. TCP 18096 仍保留

socket 入口只给 fnOS 桌面用；电视端 App 锁死直连 `IP:18096`，两者共存，不冲突。

### 5. 默认密码

admin/admin（明文写入 config.yaml，二进制启动时自动 hash 回写）。装机后 admin 登录用明文 admin。

---

## 遗留风险

1. **JS 正则重写的覆盖面**：admin.js 里如果未来新增其他绝对路径 API 调用（不在当前 5 类 pattern 覆盖范围内），sidecar 不会重写 → 那个 API 会 404。当前 admin.js 已全量扫过，所有绝对路径都在覆盖范围内。如上游更新 admin SPA，需重新审计。

2. **SSE/WebSocket**：emby-in-one 的 admin 后台目前未发现 SSE/WebSocket 连接（不像 hyatlas 有 SSE）。如果后端未来加 SSE，sidecar 的流式泵已就绪（照搬 hyatlas），但未经 emby 实测。

3. **status 双活误判**：fnOS keepalive 周期性调 `cmd/main status`，如果 sidecar 临时卡顿（python GIL），可能误报「停用」触发自动 start。pgrep 兜底已降低此风险，但不能完全消除。实测 hyatlas 同款逻辑运行稳定。

4. **二进制 buildinfo 版本**：本次只改打包层（cmd/main + sidecar + ui/config + manifest），未重编 Go 二进制。二进制 buildinfo 仍报 1.4.4-（-2 轮次编译的产物），manifest 报 1.4.4-3。如需二进制 buildinfo 也报 1.4.4-3，需重编（当前无 Go 源码在打包源，二进制是上游预编译产物）。
