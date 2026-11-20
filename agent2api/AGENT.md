# agent2api — 子项目 AGENT 须知

> 通用规矩见根 [AGENT.md](../AGENT.md)。本文件讲 agent2api 特有的打包形态与坑。

## 定位

把多家 AI 桌面客户端的登录态包装成本地 OpenAI 兼容 API 网关，自带多语言管理面板。上游 = [aimod-cc/agent2api](https://github.com/aimod-cc/agent2api)，**闭源发行包**——本仓库只维护 fnOS 打包层，**不 fork 源码**（区别于 hyatlas / wechat-on-cloud）。

本机现役端口 **3065**（loopback 用 `127.0.0.1`），桌面入口 iframe 直连 `3065`，手机 `/app/Agent2API`。

## 打包层构成（本仓库 `agent2api/`）

- `manifest`：appname=`Agent2API`、version、changelog、`checksum`（上游 app.tgz 校验，非 fpk sha256）
- `cmd/` 9 脚本：install/config/main/uninstall/upgrade × init/callback。**缺任一 → fnpack build 10111**
- `config/privilege`（run-as package）+ `config/resource`（当前空 `{}`，数据落 @appdata 不走 share）
- `wizard/install`、`wizard/config`：单字段 `port`（默认 3065，required）
- `wizard/uninstall`：`wizard_data_action`（keep/delete，默认 keep）
- `app/ui/config`：桌面入口 iframe + `port` 模式（**无 gatewaySocket**，属端口直连型，见 vikunja v13 结论）
- `app/ui/images/`：入口图标 icon_64 / icon_256

## 二进制与 fpk（不入库）

- `app/server/agent2api-server`（~36MB Rust 单文件）+ `app/server/ui/`（管理面板）+ `app/server/install.sh`、`agent2api.service`、`agent2api.env.example` —— 由上游发行包 `beta.tar.gz` 解包得到，**落打包目录 `app/server/` 后 `fnpack build`**⚠️ fnpack 只收 `app/` 下的程序与资源，根级 `server/` 不入包（曾踩坑：根级 server/ 打出的 fpk 只有 43KB）。均不进 git。
- `Agent2API.fpk`（~23.8MB）走 GitHub Release 分发，`.gitignore` 的 `*.fpk` 已覆盖。
- ⚠️ 别把 `server.bak_beta*` 之类留底目录复制进仓库（上游 beta 迭代快，留底留在 `/vol2/1000/workspace/a2a-beta-build/`）。

## cmd/main 要点

- 导出 `AGENT2API_HOST=0.0.0.0`、`AGENT2API_PROXY_PORT=$port`、`AGENT2API_PROXY_HOME=${TRIM_PKGVAR}`、`AGENT2API_UI_DIR=${TRIM_APPDEST}/server/ui`、`AGENT2API_CAPTCHA_ENABLED=0`
- 起的程序 = `${TRIM_APPDEST}/server/agent2api-server`
- 日志 `${TRIM_PKGVAR}/info.log`、PID `${TRIM_PKGVAR}/app.pid`
- `status` 先判活再 start（幂等）

## 数据与路径

- 数据（SQLite + 配置 + 日志）落 `TRIM_PKGVAR`（@appdata），**不声明 data-share**——与 hyatlas / vikunja 的 share 合规路线不同，别照搬两跳解析。
- 卸载 `wizard_data_action=delete` 才清 `TRIM_PKGVAR`；keep 全保留。

## 版本口径

上游 beta 版 → `<上游版本>-N`。当前 **3.5.0-4** = 上游 v3.5.0-beta.4。上游迭代快（beta.1→beta.2→beta.3 同日内），每次换包只动 `app/server/`（二进制、面板、install.sh 等）+ `manifest`（version / changelog / checksum），打包层脚本一般零改动。构建在 git 仓库外（如 `/vol2/1000/workspace/a2a-beta-build/FPK/`）执行更稳。

## 凭证红线

网关 API Key / 管理员口令属凭证：只落 `~/.dsh/credentials-vault/hermes.env`，禁写进本仓库任何文件、日志、会话。读 env 先管道脱敏。
