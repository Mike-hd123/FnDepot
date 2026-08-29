# octopus.fpk — Octopus 安装包

- 版本：0.12.1
- 打包时间：2026-08-29
- 上游项目：[bestruirui/octopus](https://github.com/bestruirui/octopus)
- 源码：本目录 `src/`（上游源码原样）
- 打包方式：fnOS `fnpack build`

## 打包说明

1. 官方 0.12.1 linux-amd64 release 二进制（22MB，前端已嵌入），未修改。
2. fnOS 原生 systemd 化：cmd/ 生命周期脚本 + install_callback 写单元，`OCTOPUS_SERVER_PORT=8081`（避开云微 8080）。
3. 数据目录 `/usr/local/apps/@appcenter/octopus/data/`，SQLite 存储。

## 安装

fnOS 应用中心添加外部源 `https://github.com/Mike-hd123/FnDepot` 后安装，或直接下载本 fpk 手动安装。
