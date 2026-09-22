# vikunja (待办) — Agent 须知

> 项目说明见 [README.md](./README.md)。本文件只讲 agent 该做什么 / 红线 / 坑。

## 定位
上游 [go-vikunja/vikunja](https://github.com/go-vikunja/vikunja)（官方 Go 重写版）v2.6.0 的 fnOS 发行：自托管待办面板。当前发布 **2.6.0-14**，桌面端口 **3456** + 手机 `/app/vikunja` 双通道。

## 架构
- 上游 release 资产解压得**静态 ELF**（59MB，not a dynamic executable），前端 SPA 打进二进制——**不改上游源码**，本地价值=打包层（`manifest` 在应用根，`src/` 存档）
- **原生 systemd 模式**（抄 octopus）：cmd/main 管 systemd unit，`User=hermes-studio`；SQLite 单文件（`/vol2/@appdata/vikunja/vikunja.db`），`.backup` 一行全量备份，常驻 ~96MB
- API token `tk_` 全自动读写（Hermes 提醒引擎用它增删改查）；reminder + 逾期 Webhook；CalDAV 可被手机订阅

## 坑
- 用户说「待办没了/不显示」指**本软件**，不是 Hermes reminders.json（两套系统易混）。排查序：查 DB 条数（`tasks WHERE deleted_at IS NULL`）→ 查服务是否刚重启（boot.log/systemd）→ 数据在=前端硬刷新
- 表列名：tasks 用 `done/deleted_at/project_id`；projects 用 `title`（不是 name）
- 与根 README 的提醒系统区分：reminders.json（26 条节日/生日/工资）走 reminders cron，跟 Vikunja 不联动

## 红线
同根 [AGENT.md](../AGENT.md)。操作细节 → Hermes 技能 `vikunja-ops`。
