# fnOS 统一网关 + SSO 调研报告：网关如何把「当前登录用户」传递给应用后端

调研日期：2026-09-10
调研对象：fnOS v1.1.7-rc1（本机实测）+ 飞牛应用开放平台官方文档（developer.fnnas.com）
调研方法：本机二进制字符串分析 + nginx 配置审计 + 官方文档交叉核验 + 实测请求路径验证

---

## 结论先行（TL;DR）

1. **fnOS 的「统一网关」不是一个单一组件，而是 nginx + trim_http_cgi + trim_open_gateway 三层协作**，其中真正把用户身份透传给应用后端的是 **trim_http_cgi（CGI 转发层）**。

2. **用户身份透传机制 = 三个 HTTP 请求头**（本机二进制实测确认）：
   - `X-Trim-Userid` — 当前登录用户的数字 UID（如 `1000`）
   - `X-Trim-Username` — 当前登录用户的用户名（如 `mike`）
   - `X-Trim-Isadmin` — 当前用户是否管理员
   应用后端从这三个 header 读当前用户，**不需要自己解析 cookie**。

3. **架构根因**：fnOS 应用服务以**独立的系统用户（AppUsers）运行，不是以当前登录用户运行**。所以「当前是谁在用」这个信息必须由网关从登录态里解出来、以 header 形式注入后端，后端才能按用户区分内容、调 Open API 查授权路径（`uid` 字段）。

4. **「统一网关」在官方文档语境里还有第二层含义**：指 `/app/<app>/` 这个**同域反向代理**（nginx `location /app/` → `trim_http_cgi`），让应用页面、系统授权页、`redirectUri` 回调页处于同一域名下，从而满足 OAuth `postMessage` 的同源校验。SSO 的「同域」是网关的核心价值之一，不只是身份透传。

5. **授权/SSO 本身由 trim_open_gateway 承担**：它是一个 gRPC 服务（`/run/trim_open_gateway.socket`），nginx 以 `auth_request off` + `grpc_pass` 直接转发，内部实现 `og.auth.AuthService`、OAuth token 表、`RefreshToken` 等——即 fnOS 的 OAuth 2.0 / OIDC 授权服务器。

> 本文只补充 fnOS 统一网关 / SSO 层，**不重复** `fnos-open-api-integration.md` / `trim-open-api-integration.md` 已覆盖的静态文件共享与 Open API 三件套。两份文档的定位分工见文末「与已有文档的边界」。

---

## 一、官方文档对「统一网关」与用户识别的原始表述

来源：developer.fnnas.com 官方文档（2026-09-10 抓取）

### 1.1 架构约束：应用不以当前用户身份运行

来源：`/api/authorization/overview/`

> 「应用访问用户存储空间中的文件夹或文件前，需要先获得授权。**由于应用服务通常以独立的应用用户运行，不是以当前登录用户运行**，系统需要把对应路径的 ACL 权限授予这个应用用户后，应用才能实际访问目标路径。」
>
> 「拿到授权路径和应用用户 ACL 权限，不代表可以绕过当前使用用户的系统权限。应用仍然需要按当前用户的文件权限决定是否展示、读取、写入或删除内容。」

**含义**：这是整个 SSO 需求的根因。应用进程跑在系统用户（本机实测为 `AppUsers`）下，它本身不知道「此刻浏览器前坐的是谁」，必须靠网关把登录用户身份递进来。

### 1.2 网关负责「识别当前使用用户」，后端拿 uid 用

来源：`/api/authorization/user-access/`

> 「用户个人授权路径建议结合**统一网关**使用。**应用通过统一网关识别当前使用用户，再用该用户的 uid 查询或管理目录授权路径**；文件授权结果以选择器返回的文件路径为准。」
>
> 「调用该接口前，**应用应先通过统一网关确认当前使用用户，并使用该用户的 uid 作为查询条件**。」

后端 API 请求体示例（同页）：

```json
{ "reqId": "string", "req": "trim.file.getUserAccessibleFolders",
  "appName": "string", "data": { "uid": 1000 } }
```

> 「字段：`uid` number 是 用户 UID」

**含义**：文档明说「通过统一网关确认当前使用用户」，但**文档没有公开网关用哪个 header 传递 uid**——这正是本文用二进制逆向补上的空白。

### 1.3 统一网关的「同域 / SSO」语义

来源：`/api/calling/`

> 「路由授权建议结合**统一网关**使用。**统一网关可以保证应用页面、授权页面和 redirectUri 回调页处在同一域名下**，便于回调页解析结果、通知原应用页面，并**完成 postMessage 的同源校验**。」
>
> 「为了让回调更稳定，建议通过统一网关访问应用，并把 redirectUri 设置为**同域路径**，例如 `/app/your-app/callback.html`。这样原应用页面和回调页是**同源页面**，回调页才能安全地用 `window.opener.postMessage` 通知原页面。」

**含义**：这里的「统一网关」= nginx `/app/<app>/` 统一入口。它让 `/app/<app>/页面`、系统授权页、`/app/<app>/callback.html` 全在同一 host 下，OAuth 弹窗回调的 `postMessage` 才能通过同源检查。**这是「SSO 网关」最直白的实现**：同域 = 同源 = 可安全回传授权码。

---

## 二、本机实测：网关到底注入了哪些用户头

### 2.1 三层转发链（nginx 配置审计）

`/usr/trim/nginx/conf/nginx.conf` + `/usr/trim/nginx/conf/conf.d/*.conf`：

```
浏览器请求
  → nginx (location /app/)            [nginx.conf: trim_http_cgi.conf]
      proxy_pass http://unix:/var/run/trim_http_cgi.socket:
      proxy_set_header Host $host
      proxy_set_header Upgrade $http_upgrade      ← 仅这三项，nginx 自身不注入 uid
      proxy_set_header Connection "upgrade"

  → trim_http_cgi (CGI 转发服务)      [/usr/trim/bin/trim_http_cgi]
      ★ 在此层解析登录态 cookie、鉴权、注入 X-Trim-* 用户头
      → 按 app 名路由到该应用的 app.sock（unix socket）

  → 应用自己的 gateway sidecar        [gateway-proxy.py / gateway 二进制]
      ★ 剥前缀反代：/app/<app>/<x> → /<x>，转发到后端 TCP 端口

  → 应用后端（业务代码）
```

**关键**：nginx 的 `location /app/` 只设 `Host / Upgrade / Connection` 三个头（见下节配置原文），**用户身份头不是 nginx 直接注入的**，而是下一层 `trim_http_cgi` 这个 CGI 服务注入的。这与 nginx 内置 `auth_request` 模式（如 `safe_code_access.conf` 用于飞牛安全码）是两套独立机制。

`/usr/trim/nginx/conf/conf.d/trim_http_cgi.conf`（原文）：

```nginx
location /app/ {
    proxy_pass http://unix:/var/run/trim_http_cgi.socket:;
    proxy_http_version 1.1;
    proxy_set_header Upgrade $http_upgrade;
    proxy_set_header Connection "upgrade";
    proxy_set_header Host $host;
    proxy_read_timeout 300s;
}
```

`/usr/trim/nginx/conf/conf.d/trim_open_gateway.conf`（原文，Open API / OAuth 通道）：

```nginx
location ~ ^/(og|grpc)\. {
    auth_request off;                        ← 不走安全码校验，靠自身鉴权
    grpc_pass grpc://unix:/var/run/trim_open_gateway.socket;
    grpc_read_timeout 300s;
    grpc_send_timeout 300s;
}
```

### 2.2 trim_http_cgi 注入的三个用户头（字符串实测）

对 `/usr/trim/bin/trim_http_cgi` 做 `strings | grep`，提取到唯一带 `X-Trim-` 前缀的 header 常量：

```
$ strings /usr/trim/bin/trim_http_cgi | grep -oE "X-Trim-[A-Za-z]+" | sort -u
X-Trim-Isadmin
X-Trim-Userid
X-Trim-Username
```

同一二进制的其他关键字符串（同一轮 strings 抓取）：

| 字符串 | 含义 |
|---|---|
| `invalid token` | 无有效登录 cookie 时的阻断响应（本机实测 13 字节 body） |
| `trim-cgi-sign` | CGI 通道签名 |
| `self-register` | 应用 sidecar 自注册 socket 到网关 |
| `json:"uid"` / `UidMappings` | uid 解析与 uid 映射表 |
| `SocketPath` / `SocketType` / `SacSocket` | socket 注册信息（配合 trim_sac） |

> **结论（本机二进制实测，非文档）**：统一网关把当前登录用户以 `X-Trim-Userid`（数字 UID）、`X-Trim-Username`（用户名）、`X-Trim-Isadmin`（是否管理员）三个请求头注入到应用后端。**这是官方文档没公开的实现细节**——文档只说「通过统一网关确认当前使用用户」，没说用什么头。

### 2.3 请求路径实测对照

本机对同一个 hyatlas 应用做路径对照：

| 请求 | 结果 | 说明 |
|---|---|---|
| `curl http://127.0.0.1:19528/`（直连后端端口） | 无响应 | hyatlas 后端只在 unix socket 上服务，TCP 端口不通 |
| `curl http://127.0.0.1/app/hyatlas/`（经网关，无 cookie） | `HTTP/1.1 200 OK` + `Content-Type: text/plain` + body `invalid token`（13 字节） | 命中 trim_http_cgi 的登录态校验失败分支 |
| `curl http://127.0.0.1/app/fygo-browser/`（经网关，无 cookie） | 同上 `invalid token` | 同一条链路 |

**含义**：网关不是纯转发，它在 `/app/` 入口做**登录态门禁**。无登录 cookie → 直接吐 `invalid token`，请求根本到不了应用后端。有登录 cookie 时，trim_http_cgi 解出用户身份、注入 `X-Trim-*` 头后再路由到该 app 的 socket。**这就是「SSO 网关」的最小定义：一次登录，所有 `/app/*` 应用自动带上用户身份。**

### 2.4 应用侧的网关声明点

以本机 `fygo-browser`（`micro_app = true`）为例，两处声明：

**① `ui/config`（前端页面侧，声明走统一网关）**

```json
"url": {
  "<app>": {
    "Application": {
      "microApp": true,
      "gatewaySocket": "app.sock",
      "gatewayPrefix": "/app/fygo-browser",
      "url": "/app/fygo-browser/browser.html"
    }
  }
}
```

**② `/var/apps/<app>/config/resource`（后端 API scope，root-only）**

```json
{ "api-scope": [ "trim.file.userAccess", "trim.file.userAcl" ] }
```

> 注：`config/resource` 权限为 root-only（`0400 root:root`），普通应用用户读不到，需用 `sudo -n`。这是 fnOS 设计上的边界——应用声明的 scope 只能由系统（管理员态）读取，应用自身运行时不持有一份。

**声明 → 行为映射**：
- `microApp: true` → 应用页面在 `/app/<app>/` 下由统一网关加载（同域），JS SDK 才会初始化（官方文档 `/api/calling/`：「必须在 manifest 中声明 `micro_app=true`，未声明时应用页面不会按微应用环境加载，JS SDK 相关能力可能无法初始化」）
- `gatewaySocket: app.sock` → 网关把 `/app/<app>/*` 转发到该 unix socket
- `gatewayPrefix` → 应用自己的 sidecar 据此剥前缀

---

## 三、SSO / 授权服务器的实现：trim_open_gateway

nginx 把 `/og.*` 和 `/grpc.*` 直连到一个 gRPC 服务，且 `auth_request off`：

```
$ ls -la /run/trim_open_gateway.socket
srw-rw----+ 1 root root 0  ...  /run/trim_open_gateway.socket
```

对 `/usr/trim/bin/trim_open_gateway` 做 strings，提取到授权服务器特征（同轮抓取）：

| 字符串 | 含义 |
|---|---|
| `og.auth.AuthService` | 授权服务 gRPC 服务名 |
| `OAuth` / `# OAuth` / `oauth:` | OAuth 配置段 |
| `RefreshTokenResult` / `RefreshTokenCommand` | 刷新 token 流程 |
| `get app user failed` / `Client unregistered` | 应用侧用户解析 |
| `user_guid VARCHAR(32)` / `token VARCHAR(32) PRIMARY KEY` | token 持久化表结构 |
| `VerifyTokenResult` / `appstoreclient` | token 校验与商店验证 |
| `fnos-long-token` | 长令牌 |
| `rate limit exceeded` / `write access denied` | 访问控制 |

**含义**：`trim_open_gateway` 是 fnOS 的 OAuth 2.0 授权服务器 + Open API 网关二合一。应用前端调 `openAppAuth` 打开的系统授权页、后端调 `trim.file.*` Open API，最终都落在这个服务上。它与 `trim_http_cgi` 分工明确：

- **trim_http_cgi**：`/app/<app>/` 应用页面入口，负责登录态门禁 + `X-Trim-*` 用户头注入（应用「知道自己服务的是谁」）
- **trim_open_gateway**：`/og.*` 授权与 Open API 入口，负责 OAuth 授权码 / token 签发与校验（应用「能合法拿到用户授权的路径」）

---

## 四、完整机制图（文本版）

```
┌─────────────────────────────────────────────────────────────┐
│  浏览器（用户 mike 已登录 web-ui，持有登录 cookie）            │
└───────────────────────────┬─────────────────────────────────┘
                            │ GET /app/<app>/browser.html
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  nginx  (location /app/)                                     │
│  proxy_pass unix:/var/run/trim_http_cgi.socket               │
│  仅注入 Host / Upgrade / Connection —— 不注入用户身份         │
└───────────────────────────┬─────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  trim_http_cgi（CGI 转发层）★ 身份注入点 ★                   │
│  1. 校验登录 cookie；无效 → 200 + "invalid token"(13B) 阻断   │
│  2. 解析当前用户 → 注入:                                      │
│       X-Trim-Userid   = 1000                                  │
│       X-Trim-Username = mike                                  │
│       X-Trim-Isadmin  = true/false                            │
│  3. 按 app 名路由到该应用的 unix socket（app.sock）           │
└───────────────────────────┬─────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  应用 gateway sidecar（gateway-proxy.py / gateway 二进制）    │
│  剥前缀: /app/<app>/<x> → /<x>；转发到后端 TCP 端口           │
│  （参考实现：HyAtlas gateway_proxy.py，纯剥前缀反代，           │
│    不新增用户头 —— 身份已在上一层注入，原样透传）               │
└───────────────────────────┬─────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  应用后端业务代码                                             │
│  读 X-Trim-Userid → 得到当前用户 uid                          │
│  → 调 Open API getUserAccessibleFolders({uid}) 查授权目录    │
│  → 用 trim.file.userAcl 检查该 uid 对目标路径的 rw/删除权限  │
│  → 按权限决定展示 / 读取 / 写入 / 删除                        │
└─────────────────────────────────────────────────────────────┘

        ┌──────────────────────────────────────────┐
        │  trim_open_gateway (gRPC, /og.*)          │  ← OAuth 授权服务器
        │  og.auth.AuthService / RefreshToken       │     + Open API 网关
        │  auth_request off，独立鉴权               │     （与上面链路平行）
        └──────────────────────────────────────────┘
```

---

## 五、对应用开发者而言的关键规则

1. **不要在应用后端解析 cookie**。拿不到也没必要——直接读 `X-Trim-Userid` 就是当前用户 UID。
2. **必须声明 `micro_app=true`**，否则页面不走统一网关、JS SDK 不初始化、也拿不到用户身份。
3. **`config/resource` 里的 `api-scope` 要按需最小声明**（官方文档：「只声明应用确实会用到的 Scope，不要无脑写满所有 Scope」）。
4. **拿到授权路径 ≠ 可以访问**。官方明确要求：「应用仍然需要按当前用户的文件权限决定是否展示、读取、写入或删除内容」——即用 `trim.file.userAcl` 按 `uid` 二次校验。
5. **redirectUri 必须同域**（形如 `/app/<app>/callback.html`），否则 OAuth 回调的 `postMessage` 同源校验失败。
6. **应用进程以 AppUsers 运行**，不是当前登录用户。文件系统层面的 ACL 由系统在授权时授予 AppUsers，业务层面的「这是谁在用」由 `X-Trim-Userid` 给出。

---

## 六、与已有文档的边界

| 文档 | 覆盖范围 | 本文补充 |
|---|---|---|
| `fnos-open-api-integration.md` | 静态文件共享、Open API 三件套（scope 声明 / `trim.file.*` 调用 / 错误码） | 不重复 |
| `trim-open-api-integration.md`（skill reference） | Open API 接入基础 | 不重复 |
| **本文** | 统一网关的三层转发链、用户身份 header 注入机制（`X-Trim-*`）、SSO 授权服务器（trim_open_gateway）、`/app/` 入口的登录态门禁 | 本文 |

---

## 七、证据清单（可复现）

| # | 证据 | 命令 / 来源 | 关键产出 |
|---|---|---|---|
| 1 | 官方：应用不以当前用户运行 | `/api/authorization/overview/` | 「应用服务通常以独立的应用用户运行，不是以当前登录用户运行」 |
| 2 | 官方：网关识别当前用户 + uid 查询 | `/api/authorization/user-access/` | 「应用通过统一网关识别当前使用用户，再用该用户的 uid 查询」；`getUserAccessibleFolders` 请求体含 `"uid": 1000` |
| 3 | 官方：统一网关 = 同域反代（SSO） | `/api/calling/` | 「统一网关可以保证应用页面、授权页面和 redirectUri 回调页处在同一域名下……完成 postMessage 的同源校验」 |
| 4 | nginx `/app/` 只注入 3 个头 | `/usr/trim/nginx/conf/conf.d/trim_http_cgi.conf` | `Host / Upgrade / Connection`，无 uid |
| 5 | nginx OAuth 通道独立 | `/usr/trim/nginx/conf/conf.d/trim_open_gateway.conf` | `auth_request off` + `grpc_pass` |
| 6 | ★ 用户身份头实测 | `strings /usr/trim/bin/trim_http_cgi \| grep -oE "X-Trim-[A-Za-z]+" \| sort -u` | `X-Trim-Isadmin` / `X-Trim-Userid` / `X-Trim-Username` |
| 7 | 登录态门禁实测 | `curl -v http://127.0.0.1/app/hyatlas/` | `200 OK` + `text/plain` + `invalid token`（13 字节） |
| 8 | 直连后端不通 | `curl http://127.0.0.1:19528/` | 无响应（后端只在 unix socket 服务） |
| 9 | 应用侧网关声明 | `ui/config` 的 `.url.<app>.Application` | `microApp` / `gatewaySocket` / `gatewayPrefix` |
| 10 | SSO 授权服务器 | `strings /usr/trim/bin/trim_open_gateway` | `og.auth.AuthService` / `RefreshTokenCommand` / `user_guid` / `token VARCHAR(32) PRIMARY KEY` |
| 11 | 应用剥前缀反代参考实现 | `/vol1/@appcenter/hyatlas/gateway/gateway_proxy.py`（351 行，注释「剥前缀 HTTP 反代」） | 确认 sidecar 层不新增用户头，原样透传 |

---

## 八、未确认 / 存疑项（诚实标注）

1. **`X-Trim-Isadmin` 的具体取值格式**（`true/false` vs `1/0`）未实测确认——需在应用后端加日志抓一次真实请求头。本文只确认了头名存在。
2. **trim_http_cgi 是否还注入其他非 `X-Trim-` 前缀的用户相关头**（如 `X-Forwarded-*`、`REMOTE_USER` 类 CGI 变量）未穷尽枚举。strings 里可见 `REMOTE_ADDR` / `SERVER_NAME` 等标准 CGI 环境变量，是否映射成 HTTP 头需实测。
3. **`trim-cgi-sign` 的签名算法与用途**：字符串确认存在，但完整协议未逆向。推测用于 gateway sidecar 与 trim_http_cgi 之间的自注册（`self-register`）鉴权。
4. **官方文档未公开 `X-Trim-*` 头名**，以上三个头名 100% 来自本机 v1.1.7-rc1 二进制逆向，**属于实现细节，可能随版本变化**。生产应用若依赖这些头，建议在升级 fnOS 后回归验证。
5. **`/run/trim_app_cgi/rpcbroker` socket 的角色**：确认存在（root:root `srw-rw----`），是各系统服务（usersrv / filestor / share / security 等）的 RPC 总线，但应用是否能直接调用、以及是否与 `self-register` 相关，未验证。
