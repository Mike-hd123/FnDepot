# Vikunja — 待办管理面板（v2.6.0 · x86）

飞牛 fnOS 原生应用封装：自托管待办，Go 单二进制**静态链接**（零 GLIBC 依赖），SQLite 存储。

## 上级项目

- **上游**: [go-vikunja/vikunja](https://github.com/go-vikunja/vikunja) v2.6.0（Vikunja 官方 Go 重写）
- **发布者**: Mike（fnOS 打包发行，非上游开发者）
- 上游 release 资产 `vikunja-v2.6.0-linux-amd64-full.zip`，解压得静态 ELF `vikunja`（59MB，实测 `not a dynamic executable`）

## 功能特性

- **单二进制 + SQLite**：59MB 静态 ELF，数据单文件 `.backup` 一行全量备份；常驻 ~96MB
- **中文 UI**：自带完整 SPA 前端（打进二进制），列表/看板/甘特/表格/日历
- **API token**：`tk_` 全自动读写（Hermes 用独立 token 增/查/改/删）
- **提醒引擎**：reminder（due 前 N 天/时）+ 逾期 Webhook 事件白名单
- **CalDAV**：可被手机日历/待办 App 订阅

## 打包形态

```ini
appname               = vikunja
version               = 2.6.0
install_type          = volume      # 数据落 /volN/@appdata（用户数据）
service_port          = 3456
maintainer            = kolaente    # 上游 owner
distributor           = Mike
```

- **原生 systemd 模式**（抄 octopus）：cmd/main 管理 systemd unit，`User=hermes-studio`
- **双通道入口**：桌面 iframe 直连 `3456`（协议相对 `://${host}:<PORT>/`）+ 手机 `/app/vikunja` 走 gateway socket 剥前缀反代（`app/gateway/gateway_proxy.py`）
- **数据落 @appdata**：`/volN/@appdata/vikunja/`（vikunja.db + files/ + config.yml），卸载保留
- **注册已关**：`enableregistration: false`，管理员 `user create` 建号，Hermes 用独立 token

## 首次安装后配置（管理员）

```bash
# 管理员建号（注册已关）
sudo -u hermes-studio /volN/@appdata/vikunja/vikunja user create --config /volN/@appdata/vikunja/config.yml -u mike -e mike@qq.com -p '<pwd>'
sudo -u hermes-studio /volN/@appdata/vikunja/vikunja user set-admin --config /volN/@appdata/vikunja/config.yml mike --admin

# Hermes API token（最小权限：tasks 读写 + projects 读）
# POST /api/v1/tokens → 存 .env VIKUNJA_API_TOKEN
```

## 构建

`src/` 含打包关键文件（manifest / ui-config / gateway_proxy.py）。完整 build 流程：

```bash
# 1. 下载官方 release 并验 sha256（zip 内自带 .sha256）
# 2. 二进制备份到 project/app/vikunja
# 3. ui/config + gateway + 图标
# 4. fnpack build
export PATH=/vol1/@appcenter/nodejs_v24/bin:$PATH
cd project && fnpack build -d .
```

## 与本机 Hermes 集成（2026-09-06 实证）

- `VIKUNJA_API_TOKEN` 存 `hermes-home/.env`（不入代码）
- `scripts/vikunja_client.py`：建任务/标 done/查逾期（自然语言日期解析）
- `scripts/festival_seed_vikunja.py`：农历/节气 sxtwl 转公历建年度任务（每年 11-01 cron）
- `scripts/vikunja_notify.py`：节律推送（09:00 到期 / 14:00+19:00 逾期追缴）
- 4 条 cron 接入 default jobs.json，deliver weixin

## 验证记录

- 静态 ELF sha256 与官方 .sha256 一致
- `/api/v1/info` 200，`/` 完整 SPA
- 建号/建项目/建任务/设 reminder/查 filter/标 done/建 tk_/建 webhook 全 200~201
- Webhook 目标**必须公网 IP**：Vikunja 拒所有私有网段（127.0.0.0/8、192.168.0.0/16）实测
## v9 变更（2026-09-07）

- **修复**：网关路径 `/app/vikunja` 下 vue-router history base 硬编码 `/`，站内跳转产生裸路径（地址栏丢前缀），刷新掉回 fnOS 根页面。v8 的 SPA shim（history.replaceState 剥前缀）方案废弃——路由器会采纳剥过的路径且不回写。
- **新方案**：sidecar 对带前缀的 JS 流量动态改写 router 工厂 base（`history:<fn>(\`/\`)` → `history:<fn>(\`/app/vikunja/\`)`，锚点唯一），桌面直连（无前缀）流量原样透传。
- **sidecar 运行形态**：nohup 裸跑 → systemd 单元 `vikunja-sidecar`（BindsTo=vikunja，开机自启/崩溃自拉，ExecStartPre 清理 v8 遗留 gateway.pid）。
- **desktop_applaunchname** 对齐 `vikunja.panel`（fnpack 强校验 ui/config 入口名一致）。
- **安装脚本卷探测**：install_callback 可能不带 TRIM_* env 被调用，APPDEST/DATA_DIR 缺失时扫描 /vol1-4 实际落点，不写死卷。
- 实测：安装器原生写入 `trim_sac.entry.micro_app=t` + `appcenter.app.micro_app=true`（v8 首装未触发，v9 正常）。

## v10 变更（2026-09-07，t_de3a27dc 实证）

- **根因**：vite 动态 import chunk 的 base 工厂硬编码根绝对路径 `` return`/`+e ``（preload-helper 把懒加载 chunk/样式拼成 `http://<host>/assets/...`），经网关丢 `/app/vikunja` 前缀 → 全 404 → `Unable to preload CSS` 中断启动 → SPA 卡“Vikunja 正在加载…”/部分加载失败。
- **修复（sidecar 网关流量三层补丁，桌面直连零影响）**：
  1. **JS preload base**：锚点 `` return`/`+e `` → `` return`../`+e ``（模块相对，网关下解析 `/app/vikunja/assets/`，桌面直连解析 `/assets/`）。
  2. **CSS 绝对资源**：`url(/assets/...)` → `url(/app/vikunja/assets/...)`（字体/背景图，静态文件网关 301 会掉前缀）。
  3. 保留 v9 的 router base 改写。
- **验证（CDP 真实登录 + JWT 注入，全部通过）**：首页海报 200→首页渲染（项目/任务/概览）→ SPA 深路由 `/tasks/3` 详情→ 编辑→ Tiptap 富文本编辑器（标题/粗体/斜体/表格工具栏）→ 深路由硬刷新不掉根、零 404、零 JS 异常。直连 3456 桌面模式 preload 仍 `return`/`+e`、CSS 未打前缀。
- **skill 已加**：vite 动态 chunk 网关前缀丢失专节 + JWT HS256 会话自签登录法（见 fnos-app-package）。
