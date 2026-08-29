# waliapi.fpk — WaLiAPI 安装包

- 版本：0.2.5
- 打包时间：2026-08-30
- 上游项目：[fuzhengwei/WaLiAPI](https://github.com/fuzhengwei/WaLiAPI)
- 源码：本目录 `src/`（上游源码原样，Tauri 2 / Rust + React，MIT）
- 打包方式：fnOS `fnpack build`

## 打包说明

1. 官方 docker 镜像 `fuzhengwei/waliapi:0.2.5-amd64`（bookworm 基座）内 headless 服务二进制 `waliapi-web`（30MB，GLIBC≤2.34，NAS 2.36 兼容），未修改。GitHub release 的 deb/AppImage 均为 GLIBC_2.39 构建，NAS 跑不了，故取镜像内二进制。
2. Web 管理面板资源内嵌于二进制，无需独立 ui 目录。
3. 运行库回调：webkit2gtk-4.1 三件套 + GTK3 + 传递依赖约 20 个 so 打入 `app/lib/`（bookworm 版本），`LD_LIBRARY_PATH` 指向自带库，宿主机零安装依赖。
4. fnOS 原生化：`cmd/main` 生命周期（start/stop/status），端口 **8777**（避开云微 8080 / octopus 8081），数据目录 `app/data`（SQLite），日志/运行态在 `/var/apps/waliapi/var/`。
5. 首次启动初始密码写入 `data/INITIAL_PASSWORD`（日志同时打印）。

## 安装

fnOS 应用中心添加外部源 `https://github.com/Mike-hd123/FnDepot` 后安装，或直接下载本 fpk 手动安装。

## 已验证（2026-08-30, fnOS 实机）

- appcenter-cli install-fpk + start：`/health` 200，Web 面板 iframe 入口正常（图标/标题）
- 端口无冲突：8080（云微）/ 8081（octopus 预留）不受影响
