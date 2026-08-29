# WaLiAPI Web 管理面板

Docker 部署为纯 Web 形态：独立 headless 二进制 `waliapi-web`（无桌面窗口、无显示服务器），内嵌管理面板，浏览器直接访问。页面与桌面端 1:1 一致（复用同一套 React 源码）。

## 架构

```
浏览器
  │ HTTPS
  ▼
nginx (docker-compose, :8443)
  │ /* → waliapi:8777
  ▼
waliapi-web（headless 进程, :8777）
  ├─ /v1/*              LLM 网关协议（API Key 鉴权，宽松 CORS）
  ├─ /api/kb/* /api/wiki/* /mcp   知识库 / Wiki / MCP
  ├─ /admin/api/*       管理 REST API（会话鉴权 + CSRF 防护，无 CORS 放行）
  │   ├─ /auth/login | logout | check | change-password | change-username
  │   ├─ /invoke        与 Tauri invoke 语义 1:1 的命令入口
  │   └─ /events        SSE 事件桥（KB/Wiki 进度事件）
  └─ /*                 rust-embed 内嵌的 Web 静态资源（SPA fallback）
```

- 双二进制：`waliapi`（桌面端，Tauri 窗口）与 `waliapi-web`（headless 服务，`waliapi-web start`），共享同一套业务代码。
- 前端：`web/` 是 pnpm workspace 子包，通过 vite alias 复用 `src/` 全部页面组件，仅把 `@tauri-apps/api/*` 替换为 HTTP fetch / EventSource 实现。
- 后端：`/admin/api/invoke` 按命令名直接分发到现有 commands 函数，桌面端与 Web 端共用同一套业务逻辑。
- 认证：SQLite `admin_users` 表 + argon2id 密码哈希；会话为内存 Bearer token（7 天），同时写 `waliapi_admin_token` Cookie 供 SSE 使用；进程重启会话失效。

## 启动方式

### 桌面版（自动启动服务 + 桌面窗口，不含 Web 管理面板）

```bash
waliapi        # 桌面窗口；启动时自动拉起内嵌后端服务（网关 + /admin/api）
```

桌面版包含桌面前端 + 后端服务，启动即自动运行。**Web 管理面板 SPA 不随桌面版提供**（桌面构建不含 `embed-web`，浏览器访问桌面实例的 `/` 返回 404）；如需浏览器管理，请使用 `waliapi-web` 或 Docker。

### 独立 Web 服务（headless）

```bash
waliapi-web                                    # 直接启动（无需子命令）
waliapi-web start [--host 0.0.0.0] [--port 8777] [--data-dir /data]
```

环境变量等价物：`WALIAPI_SERVER_HOST` / `WALIAPI_SERVER_PORT` / `WALIAPI_DATA_DIR`。
数据目录缺省解析：`--data-dir` > `WALIAPI_DATA_DIR` > `$XDG_DATA_HOME/waliapi.xiaofuge.cn` > 平台应用数据目录（与桌面端一致，数据互通）。

### Docker（默认启动 Web 服务）

镜像内只含 `waliapi-web`（`ENTRYPOINT ["waliapi-web"] CMD ["start"]`），`docker run` 无需任何参数即启动完整服务（后端 + Web 管理面板），不含桌面前端与显示服务器。访问 `http://localhost:8777`。

```bash
docker build -t waliapi:local .
docker run -d -p 8777:8777 -v waliapi-data:/data --name waliapi waliapi:local
```

#### 构建参考（阿里源加速）

```bash
docker build --build-arg NODE_IMAGE=docker.m.daocloud.io/library/node:22-bookworm --build-arg RUNTIME_IMAGE=docker.m.daocloud.io/library/debian:bookworm-slim --build-arg DEBIAN_MIRROR=http://mirrors.aliyun.com/debian --build-arg DEBIAN_SECURITY_MIRROR=http://mirrors.aliyun.com/debian-security -t waliapi:local .
```

#### 镜像导出 / 导入（离线分发）

```bash
docker save waliapi:local -o waliapi.tar
docker load -i waliapi.tar
```

#### docker-compose（HTTPS 反代）参考

将以下两个文件保存到同一目录（另需同级 `certs/` 目录存放 `fullchain.pem` / `privkey.pem`），然后 `docker compose up -d`。

`docker-compose.yml`（按实际需求修改 `ports`）：

```yaml
services:
  waliapi:
    image: waliapi:local
    restart: unless-stopped
    environment:
      WALIAPI_SERVER_HOST: 0.0.0.0
      WALIAPI_SERVER_PORT: 8777
      XDG_DATA_HOME: /data
      TZ: Asia/Shanghai
    ports:
      - "8777:8777"
    volumes:
      - waliapi-data:/data
    healthcheck:
      test: ["CMD-SHELL", "curl -fsS http://127.0.0.1:8777/health >/dev/null"]
      interval: 30s
      timeout: 5s
      retries: 5
      start_period: 60s

  nginx:
    image: nginx:1.27-alpine
    restart: unless-stopped
    depends_on:
      waliapi:
        condition: service_healthy
    ports:
      - "8443:443"
    volumes:
      - ./nginx.conf:/etc/nginx/conf.d/default.conf:ro
      - ./certs:/etc/nginx/certs:ro

networks:
  default:
    name: waliapi_network

volumes:
  waliapi-data:
```

`nginx.conf`（`/admin/api/` 已关闭代理缓冲以支持 SSE 事件桥；按实际域名修改 `server_name`）：

```nginx
map $http_upgrade $connection_upgrade {
    default upgrade;
    '' close;
}

upstream waliapi_api {
    server waliapi:8777;
    keepalive 32;
}

server {
    listen 443 ssl http2;
    server_name yourdomain.com;

    ssl_certificate /etc/nginx/certs/fullchain.pem;
    ssl_certificate_key /etc/nginx/certs/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_prefer_server_ciphers off;

    client_max_body_size 50m;
    proxy_buffering off;
    proxy_request_buffering off;
    proxy_read_timeout 3600s;
    proxy_send_timeout 3600s;

    location ^~ /v1/ {
        proxy_pass http://waliapi_api;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
        proxy_set_header X-Forwarded-Host $host;
        proxy_set_header Connection "";
    }

    location = /health {
        proxy_pass http://waliapi_api;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
        proxy_set_header X-Forwarded-Host $host;
        proxy_set_header Connection "";
    }

    location ^~ /mcp {
        proxy_pass http://waliapi_api;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
        proxy_set_header X-Forwarded-Host $host;
        proxy_set_header Connection "";
    }

    location ^~ /admin/api/ {
        proxy_pass http://waliapi_api;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
        proxy_set_header X-Forwarded-Host $host;
        proxy_set_header Connection "";
        # SSE 事件桥（/admin/api/events）需要关闭代理缓冲
        proxy_buffering off;
        proxy_cache off;
    }

    location / {
        proxy_pass http://waliapi_api;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto https;
        proxy_set_header X-Forwarded-Host $host;
        proxy_set_header Connection "";
    }
}
```

#### 常用命令：

```bash
docker compose up -d                  # 启动
docker compose up -d --force-recreate # 强制重建
docker compose logs -f waliapi nginx  # 跟踪日志
docker compose down                   # 停止并移除
```

#### 访问地址（按实际部署调整）：

- 公网 API: `https://yourdomain.com/health`
- 局域网 API: `https://lanip:8443/health`
- Web 管理面板: `https://lanip:8443/`（或直连 `http://lanip:8777`）

#### 备注：

- 管理面板随 API 同源开放，无独立 UI 端口；首次登录的临时密码见 `docker logs waliapi` 或容器内 `/data/waliapi.xiaofuge.cn/INITIAL_PASSWORD`。
- 如果 `443` 端口已被占用，先改宿主端口映射，再同步调整穿透配置。

## 安全模型

- **登录鉴权**：`/admin/api/*` 除 `/auth/login` 外全部要求会话（Bearer token 或 Cookie）。未认证访问一律 401，任何配置/日志/统计数据都不会泄露。
- **CSRF 防护**：所有变更类方法（POST/PUT/DELETE/PATCH）必须携带 `X-Requested-With: XMLHttpRequest` 自定义头；跨域简单请求无法携带该头，跨域 fetch 携带该头会触发预检且必然失败（见下），双重拦截跨站伪造。
- **受限 CORS**：宽松 CORS（`*`）仅用于 LLM 网关 `/v1/*` 等 API（这些接口本身要求 API Key）；`/admin/api/*` 与静态资源不下发任何 CORS 允许头，浏览器跨域调用被原生拦截。
- **首次改密**：首启动生成 `admin` + 16 位随机临时密码（打印日志 + 写入 `<data-dir>/INITIAL_PASSWORD`），登录后强制修改（≥8 位）。

## 数据一致性与部署约束

- 同一实例的桌面端与浏览器访问的是**同一个内嵌服务**（同一 AppState/SQLite 连接池），数据实时一致；Web 端独有的 KB/Wiki 进度事件经 SSE 桥推送。
- **单写入者约束**：一个数据目录（SQLite 文件）同一时刻只允许一个进程运行（桌面版或 waliapi-web 二选一）。多进程同时写同一 SQLite 文件会导致锁竞争与数据损坏。
- **多人/多副本部署**：SQLite 单写入者模型不支持多副本横向扩展；如需多人并发/多副本，建议后续引入 PostgreSQL 作为外部存储（当前版本未提供，属路线图项）。

## 与桌面版的差异

| 桌面功能 | Web 版行为 |
|---|---|
| 应用更新检查 / 一键更新 | 不暴露（Web 版随镜像升级），无入口 |
| auth.json 导入（文件对话框） | 浏览器文件选择器上传内容导入 |
| auth.json 导出（保存对话框） | 浏览器直接下载 |
| 渠道导入 / 导出 | 同上：浏览器上传 / 下载 |
| 打开配置文件夹 | 容器内应用均不可用，置灰 |
| 系统托盘 / 开机自启 / 关闭到托盘 | 设置页隐藏相关开关 |
| OAuth 登录（打开系统浏览器） | headless 返回明确错误，建议用 auth.json 导入 |

## 本地开发

```bash
# 终端 1：headless 后端（内嵌已构建的 web/dist）
cd web && pnpm build && cd ..
cargo run --manifest-path src-tauri/Cargo.toml --bin waliapi-web --no-default-features --features embed-web -- start

# 终端 2：web dev server（代理 /admin/api、/api、/v1、/mcp 到 8777）
cd web && pnpm dev
```

访问 `http://localhost:1420`。桌面端开发流程（`pnpm tauri dev`）不受影响。

## 故障排查

- **无法登录 / 忘记密码**：`docker exec -it waliapi sqlite3 /data/waliapi.xiaofuge.cn/waliapi.db "DELETE FROM admin_users;"`，重启容器后重新生成临时密码。
- **SSE 进度不更新**：确认经 nginx 访问；上方 nginx 配置已对 `/admin/api/` 关闭缓冲。浏览器直连 8777 时无此问题。
- **静态资源 404 / 白屏**：镜像是用 `--features embed-web` 构建的；本地 `cargo run` 需先 `cd web && pnpm build` 生成 `web/dist`，否则 `/` 返回 404。
- **会话频繁失效**：会话存于内存，进程重启后需重新登录，属预期行为。
- **CSRF 校验失败 (403)**：自定义脚本调用 `/admin/api` 时需加请求头 `X-Requested-With: XMLHttpRequest`。
