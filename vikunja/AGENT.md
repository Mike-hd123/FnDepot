# vikunja (待办) — Agent 须知

> 项目说明见 [README.md](./README.md)。本文件只讲 agent 该做什么 / 红线 / 坑。

## 定位
上游 [go-vikunja/vikunja](https://github.com/go-vikunja/vikunja)（官方 Go 重写版）v2.7.1 的 fnOS 发行：自托管待办面板。当前发布 **2.7.1-1**（2026-10-28，同步上游 v2.7.1；本机现役曾长期停在 2.7.0-1，share 合规版 2.7.0-2 一直未装机，2.7.1-1 起一并落地），桌面端口 **3456** + 手机 `/app/vikunja` 双通道。

## 架构
- 上游 release 资产解压得**静态 ELF**（59MB，not a dynamic executable），前端 SPA 打进二进制——**不改上游源码**，本地价值=打包层（`manifest` 在应用根，`src/` 存档）
- **原生 systemd 模式**（抄 octopus）：cmd/main 管 systemd unit，unit 里的 `User` 被 `/etc/systemd/system/vikunja.service.d/override.conf` 覆盖成 `vikunja`（base unit 写 hermes-studio）；SQLite 单文件，`.backup` 一行全量备份，常驻 ~96MB
- **数据落 share（2.7.0-2 起）**：`${SHARE_ROOT}` = `/var/apps/vikunja/shares/vikunja`（软链 → `/volN/@appshare/vikunja`），DB 在 `share/data/vikunja.db`、附件 `share/data/files/`、日志 `share/data/logs/`、配置 `share/config.yml`；`@appdata` 只留 `boot.log` / `migrate.log` / PID
- API token `tk_` 全自动读写（Hermes 提醒引擎用它增删改查）；reminder + 逾期 Webhook；CalDAV 可被手机订阅

## share 合规修复（2.7.0-2，2026-10-15）
- **纠正的坑**：`config/resource` 从 2.6.0 起就声明了 `data-share(name=vikunja)`，但脚本从没用它 —— `cmd_main` 的 `DATA_DIR` 取 `TRIM_PKGVAR`(@appdata)，数据实际全在 `/vol2/@appdata/vikunja`，属**声明与实现脱节**。2.7.0-2 起四个脚本各自内联 `resolve_share_root` 两跳解析，数据/配置真身统一落 share。
- **老路径别再用**：`/vol2/@appdata/vikunja/vikunja.db` 是 2.7.0-1 及以前的位置，升级后即迁到 `share/data/vikunja.db`（老位置源文件**不删**，作保底）。查 DB 前先确认实际路径，别改错文件。
- **迁移极端保守**：只在 share 目标缺失且老位置有数据时 `cp -a`，源永不删、绝不 `mv`；记录在 `@appdata/migrate.log`。
- ⚠️ **WAL 迁移坑（2026-10-28 实测 2.7.1-1）**：迁移循环原只拷 `vikunja.db`，漏 `-wal`/`-shm`——SQLite WAL 模式下未 checkpoint 的写入只存在于 `-wal`，只搬主库 = **数据回退**（本机 22 条任务跌成 10）。已修复：install/upgrade_callback 的拷贝对增加 `vikunja.db-wal`/`-shm`（源停止后三者一起搬，sqlite 重开自动回放）。**2.7.1-1 成品包未含此修复**（本次安装靠「源不删 + 手工合并 db+wal 恢复」脱险），下次构建生效；今后升级前先 `sqlite3 <db> "PRAGMA wal_checkpoint(TRUNCATE)"` 或确认无 `-wal`。
- **卸载双选**：`wizard/uninstall` 提供 `wizard_delete_data`（默认保留），`uninstall_callback` 先停 systemd 服务再清 share 内容（`find -mindepth 1 -maxdepth 1`，含隐藏文件）。
- ⚠️ 本机 **/vol1/@appdata/vikunja 是 2026-09-09 的历史残留**（属主 hermes-studio），现役实例在 /vol2，别把它当现役数据。

## 坑
- 用户说「待办没了/不显示」指**本软件**，不是 Hermes reminders.json（两套系统易混）。排查序：查 DB 条数（`tasks WHERE deleted_at IS NULL`）→ 查服务是否刚重启（boot.log/systemd）→ 数据在=前端硬刷新
- 表列名：tasks 用 `done/deleted_at/project_id`；projects 用 `title`（不是 name）
- 与根 README 的提醒系统区分：reminders.json（26 条节日/生日/工资）走 reminders cron，跟 Vikunja 不联动

## 红线
同根 [AGENT.md](../AGENT.md)。操作细节 → Hermes 技能 `vikunja-ops`。
