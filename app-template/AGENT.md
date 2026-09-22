# app-template — Agent 须知

> 项目说明见 [README.md](./README.md)。本文件只讲 agent 该做什么 / 红线 / 坑。

## 定位
fnOS 原生应用打包**标准模板**（非应用）：从 hyatlas / octopus / wechat-on-cloud 现役结构提炼。住 FnDepot 仓库根，**不进 fnpack.json 索引、不打 fpk**。

## 用法
新应用上架 = 复制本目录 → 全局替换占位符 `<appname>` / `<上游版本>-<本地版本号>` / `<PORT>` / `<显示名>` / `<上游org>/<上游repo>` → 按 build.sh 注释填内容 → `fnpack build`。

## 结构要点
- `manifest`（INI 元信息）、`config/privilege`（run-as user 默认非 root）、`config/resource`（原生无 docker 空 {}）
- `ui/config`：桌面入口 .url iframe，**TAG 协议自适应，禁硬编码 http**
- `wizard/`：install + uninstall（保留/清除数据双选）
- `cmd/` 9 脚本必须齐全（缺任一 → 10111 / fnpack build 直接报错）；main 管 start/stop/status + PID 文件 + TRIM_PKGVAR 日志

## 红线
- 模板本身慎改：改了要回归验证 build.sh 占位符替换逻辑
- 打包部署全流程知识 → Hermes 技能 `fnos-app-package`（Docker 容器 + 原生 systemd 双模式）
