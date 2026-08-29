# FnDepot — Mike 的飞牛第三方应用源

飞牛 fnOS 外部应用源（schema_version 2），收录 4 个自打包应用。每个应用目录 = 安装包(fpk) + 图标 + 说明 + 上游源码 fork(src/)。

## 添加源

fnOS 应用中心 → 设置 → 外部应用源 → 添加：

```
https://github.com/Mike-hd123/FnDepot
```

## 应用一览

| 应用 | 版本 | 说明 | 上游项目 |
|---|---|---|---|
| 云微(飞牛云微信) `wechat-on-cloud` | 1.5.1 | NAS 原生微信面板，Node.js 面板 + dockerode 管理微信实例容器，ipvlan(woc-lan) 单网卡直连局域网，数据落主机目录 bind，移动端触屏优化 | [Gloridust/WechatOnCloud](https://github.com/Gloridust/WechatOnCloud) |
| HyAtlas(混元记忆) `hyatlas` | 2.0.1 | AI 长期记忆系统，zvec 双索引，仪表盘汉化 + token 自动登录，降权运行 | [tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory) |
| Octopus `octopus` | 0.12.1 | LLM API 聚合网关，Go 单二进制 + SQLite，端口 8081 | [bestruirui/octopus](https://github.com/bestruirui/octopus) |
| WaLiAPI `waliapi` | 0.2.5 | 多 Key API 网关聚合，单渠道多 Key 加权随机轮询，Rust 单二进制 + 内嵌 Web 面板 + SQLite，端口 8777 | [fuzhengwei/WaLiAPI](https://github.com/fuzhengwei/WaLiAPI) |

## 目录结构

```
FnDepot/
├── fnpack.json              # 源索引（V2 单文件模式，含 sha256+size）
├── README.md
├── wechat-on-cloud/
│   ├── ICON.PNG / ICON_256.PNG
│   ├── README.md
│   ├── wechat-on-cloud.fpk
│   └── src/                 # fork 上游源码 + 全部 NAS 适配改动
├── hyatlas/
│   ├── ICON.PNG / ICON_256.PNG
│   ├── README.md
│   ├── hyatlas.fpk
│   └── src/                 # 上游 HyAtlas v3.5.0 源码（汉化/在打包层另做）
└── octopus/
    ├── ICON.PNG / ICON_256.PNG
    ├── README.md
    ├── octopus.fpk
    └── src/                 # 上游 bestruirui/octopus 源码
```

注：`waliapi/` 目录结构同上（ICON / README / waliapi.fpk / src/ 上游 Tauri 源码）；打包产物 `app/`（二进制+运行库 170MB）不进 git，仅保留在 fpk 内。

## 打包说明

- 打包工具：fnOS 官方 `fnpack build -d <project-dir>`，产物内 manifest appname/version 与 fnpack.json 一致。
- 云微改动全在 `src/panel`（面板 web）与 fpk 打包层（cmd/、manifest），本仓库 `src/` 与 [Mike-hd123/WechatOnCloud](https://github.com/Gloridust/WechatOnCloud) 同步。
- HyAtlas 汉化与降权改动在 fpk 打包层（site-patches/、systemd 单元），未侵入 `src/`。
- Octopus 为官方 release 二进制原样打包。
- WaLiAPI 取官方 docker 镜像(fuzhengwei/waliapi:0.2.5-amd64, bookworm 基座)内 headless 二进制 `waliapi-web`（GitHub release 是 GLIBC_2.39 构建，NAS 2.36 不兼容），webkit2gtk+GTK 运行库回调入 `app/lib/`，`LD_LIBRARY_PATH` 自带库零宿主依赖。
- 发布者：Mike · https://github.com/Mike-hd123
