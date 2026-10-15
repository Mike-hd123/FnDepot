# wechat-on-cloud.fpk — 云微(飞牛云微信) 安装包

- 版本：1.5.2-2（飞牛包版本 = 上游版本-本地修订号；面板自报 WOC_VERSION 见下）
- 打包时间：2026-10-15（1.5.2-2 = share 合规修复）
- 面板自报版本：`cmd/main` 的 `WOC_VERSION` 固定 `1.5.2`（面板自身更新检测口径，与飞牛包版本解耦）
- 上游项目：[Gloridust/WechatOnCloud](https://github.com/Gloridust/WechatOnCloud)（fork 存档：[Mike-hd123/WechatOnCloud](https://github.com/Gloridust/WechatOnCloud)）
- 源码：本目录 `src/`（含全部 NAS 适配改动）
- 打包方式：`src/fnos-native/build.sh`（默认走定档 `app.tgz`；`--from-source` 用 `build-app.sh` 从 panel 源码产出的 `app.new.tgz`）

## 数据存放（两类数据，务必分清）

| 类别 | 内容 | 位置 | 谁管 |
| --- | --- | --- | --- |
| ① 面板数据 | 账号(accounts.json)、会话(sessions.json)、面板配置、实例日志 | **fnOS 官方共享目录 `wechat-on-cloud`**：`config/resource` 声明 `data-share` → fnOS 自动建目录并自动授 Windows ACL；面板经 `resolve_share_root` 解析出的 `SHARE_ROOT/data` 读写。PID 与运行日志另留 `@appdata` | 本应用，入官方 share |
| ② 微信实例数据 | 每个微信实例容器 `/config`（**含聊天记录**） | **用户在面板里自选的路径**：`WOC_DATA_DIR`（默认 `/vol2/1000/weixin/woc-data-<id>`，bind 形态）或 docker 命名卷 | 用户自选，**不受本应用 share 管辖** |

- 文件管理器里可见的是 ①（共享目录 `wechat-on-cloud`）；② 在你创建实例时选定的目录/卷里。
- **卸载清理只作用于 ①**：即使选「清除所有应用数据」，也只清共享目录内容（含隐藏文件）与 `@appdata`，**绝不会删 ②（微信实例数据/聊天记录）**。
- 支持 `WOC_VAR_DIR` 显式覆盖：显式设置时面板数据落该路径（自定义部署），未设置才走官方 share 解析。

## 1.5.2-2 变更（2026-10-15，share 合规修复）

- **补齐 data-share 声明**：`config/resource` 在**保留原 `api-scope: ["trim.file.sharedAccess"]`** 的前提下追加 `data-share.shares[{name:"wechat-on-cloud", permission.rw:["wechat-on-cloud"]}]`。面板数据从此由 **fnOS 官方共享目录接管**（自动建目录 + 自动授 Windows ACL），不再由脚本 `mkdir -p` + `chmod 777` 硬建（此前拿不到官方自动授权，且路径写死单卷）。
- **两跳 share 解析**：`cmd/main`、`install_callback`、`upgrade_callback`、`uninstall_callback` **各内联一份** `resolve_share_root`（优先 fnOS 维护的 `/var/apps/wechat-on-cloud/shares/wechat-on-cloud` 软链，退化用 `TRIM_DATA_SHARE_PATHS` 首段，跳过 `/vol0` 占位路径），全流程统一取 `SHARE_ROOT`，消除「两处不同来源路径」。
- **数据落点调整**：面板 config/data 落 `${SHARE_ROOT}/data`；**PID 与日志改留 `@appdata`**（`TRIM_PKGVAR`）。
- **幂等迁移（不覆盖）**：老位置 `/vol1/@appshare/wechat-on-cloud/data` 有数据、且新位置为空时才 `cp -a`；**源目录保留不删**，动作记 `migrate.log`。
- **卸载双选**：新增 `wizard/uninstall`（字段 `wizard_delete_data`，默认保留）。选清除只清共享目录内容（`find -mindepth 1 -maxdepth 1`，含 `.` 开头隐藏文件）+ `@appdata`。
- 原有职责全部保留：ipvlan woc-lan、`woc-ensure-ipvl0.sh`、TrimApiUsers 组授权、host-guard、hosts-sync、proxy、dockerode 启动逻辑等。

## 1.5.2-1 变更（2026-10-12）

- 合并上游 v1.5.2：QQ 下载实时签名校验（#153，裸链一律 403）、诊断包新增桌面服务段（nginx/KasmVNC 端口监听 + error.log）、NO_IPV6 诊断行、Admin 页删过时「QQ 只对大陆开放」文案。
- **修残包**：定档 `app.tgz` 此前缺 `tar.ts` / `upload.ts`（二者被 `docker.ts:12`、`index.ts:125` 硬 import），从仓库重建即崩；现已补回，`server/src` 下 12 个 `.ts` 齐全。
- web-dist 重建。
- `cmd/main` 的 `WOC_VERSION` 保持 `1.5.1` 未动。

## 相对上游的核心改动（全部已进 src/ 存档）

1. **ipvlan 单网卡**：删除 woc-net bridge 方案，实例容器只挂 `woc-lan`（ipvlan l2，192.168.5.0/24），面板经宿主 ipvl0 虚接口访问容器；`WOC_DOCKER_NETWORK=woc-lan` 写死默认值（cmd/main:100）。
2. **数据卷主机 bind**：`WOC_DATA_DIR` 非空时容器 /config 落主机路径 `/vol2/1000/weixin/woc-data-<id>`，否则回退 docker 命名卷。
3. **移动端触屏修复**：面板 web 不再自动抢焦点（加载/接管跳过 focusFrame，点画面才聚焦）。
4. **版本号跟随上游**：WOC_VERSION 固定 1.4.9（上游最新 tag），本地改动不变版本号。
5. **「电源」下拉**：远程链接页（实例远程桌面页）顶部按钮改「电源」下拉，含「重启」（原逻辑，重建容器数据保留）与「关机」（停止微信实例容器，数据保留可随时再启动，调已有 `POST /api/admin/instances/:id/stop`）。
6. fnOS 原生化：systemd 风格 cmd/ 脚本、wizard 安装向导、图标三路径、`PANEL_ALLOWED_HOSTS` host-guard。

## 安装

fnOS 应用中心添加外部源 `https://github.com/Mike-hd123/FnDepot` 后安装，或直接下载本 fpk 手动安装。
