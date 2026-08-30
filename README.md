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
| 9Router `9router` | 0.5.59 | FREE AI Router & Token Saver，Next.js + open-sse SSE 引擎，40+ 免费 AI 提供商聚合，端口 20128 | [decolua/9router](https://github.com/decolua/9router) |

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
├── 9router/
│   ├── ICON.PNG / ICON_256.PNG
│   ├── README.md
│   ├── 9router-0.5.59-x86.fpk
│   └── src/                 # 打包层源码（build.sh + cmd/config/wizard/manifest.upstream）
```

## 打包说明

- 打包工具：fnOS 官方 `fnpack build -d <project-dir>`，产物内 manifest appname/version 与 fnpack.json 一致。
- 云微改动全在 `src/panel`（面板 web）与 fpk 打包层（cmd/、manifest），本仓库 `src/` 与 [Mike-hd123/WechatOnCloud](https://github.com/Gloridust/WechatOnCloud) 同步。
- HyAtlas 汉化与降权改动在 fpk 打包层（site-patches/、systemd 单元），未侵入 `src/`。
- 发布者：Mike · https://github.com/Mike-hd123
