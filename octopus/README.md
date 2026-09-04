# octopus.fpk — Octopus 安装包

- 版本：0.13.2
- 打包时间：2026-09-04
- 上游项目：[bestruirui/octopus](https://github.com/bestruirui/octopus)
- 源码：本目录 `src/`（上游 v0.13.2 tag 源码原样）
- 打包方式：fnOS `fnpack build`

## 打包说明

1. 官方 0.13.2 linux-amd64 release 二进制（约 51MB，前端已嵌入，SHA256 与官方 SHA256SUMS 核对一致），未修改。新版支持单渠道多 Key。
2. fnOS 原生 volume 安装（manifest 无 install_type 行 → 落用户存储卷 `/vol1/@appcenter/octopus/`）：cmd/ 生命周期脚本 + install_callback 数据目录准备，`OCTOPUS_SERVER_PORT=8081`（避开云微 8080）。
3. 数据目录 `/vol1/@appdata/octopus/data.db`（TRIM_PKGVAR 注入，share 存储，卸载重装不丢），SQLite 存储。

## 安装

fnOS 应用中心添加外部源 `https://github.com/Mike-hd123/FnDepot` 后安装，或直接下载本 fpk 手动安装。
