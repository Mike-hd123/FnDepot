# 9Router (fnOS 原生打包)

9Router — FREE AI Router & Token Saver：AI 编码路由器，连接 Claude Code / Codex / Cursor / 其他工具 到 40+ 免费 AI 提供商。自带 Web 面板（Next.js standalone + open-sse SSE 路由引擎），支持多提供商聚合、combo、token 节省、MITM、媒体（image/tts/stt/embedding/web）全家桶。

**上游项目**: [decolua/9router](https://github.com/decolua/9router)
**本打包**: 基于上游 v0.5.65 构建的 fnOS 原生 fpk（不再依赖上游 `9router-fnos` 维护者的打包，改为 Mike 自打包）。

## 版本

- `9router-0.5.65-x86.fpk` — 上游 decolua/9router v0.5.65，本仓库重打包
  - manifest checksum: `8ebd1497e55eaf5a1e4c30bc81cff92d`
  - 文件 sha256: `3d8028e06b8cd08760673f6e470c475ad20b6ec32cdef55ed6798a0a519ac346`
  - 大小: 34,674,717 字节 (33 MB)
- 服务端口: 20128（本体监听），依赖 `nodejs_v24`（fnOS 应用中心依赖声明 `install_dep_apps = nodejs_v24`）

## 目录结构

```
9router/
├── ICON.PNG / ICON_256.PNG      # 商店图标
├── 9router-0.5.65-x86.fpk        # 打包产物
├── README.md
└── src/                          # 打包层源码存档（可复现构建）
    ├── build.sh                  # 一键构建脚本（clone 上游 v${VERSION} → standalone → fnpack）
    ├── manifest.upstream         # 上游原始 manifest（unpackaged 前维持 maintainer=decolua/techysy 的版本）
    ├── cmd/                      # fnOS 生命周期脚本（main/install/upgrade/uninstall/config × init/callback）
    ├── config/                   # privilege(run-as=package) + resource(data-share)
    ├── wizard/                   # 安装向导
    └── app-ui/                   # 桌面入口 ui/config + ui/images
```

## 构建

```bash
cd src/
./build.sh 0.5.65 x86     # 产物输出到仓库根
```

构建脚本会: clone decolua/9router@v<版本> → npm ci（npmmirror 镜像）→ `NEXT_DIST_DIR=.next-cli-build npm run build` 产出 standalone → 拷贝 open-sse / src/mitm / sql.js / node-forge 运行时依赖 → 组装 fnOS 打包结构 → fnpack build。

## 与上游打包（techysy/9router-fnos）的差异

| 项 | 上游 techysy | 本仓库 |
|----|-------------|--------|
| maintainer | decolua | **Mike** |
| maintainer_url / distributor_url | decolua/9router / techysy/9router-fnos | **mike-hd123/9router-fnos** |
| distributor | techysy | **Mike** |

同步方式：task 卡片约束「禁止 push 到 techysy 远程」，改动只走本仓库 git。

## 已知注意

- `config/privilege` run-as = **package**（应用降权运行；cmd/main 用 nohup+PID 管理进程，不依赖写系统级 systemd 单元，故无需 root）。
- manifest **无 `install_type` 行** = volume 模式，安装落用户存储空间（/vol1），不占系统分区。
- 桌面入口 `ui/config`: `9router.Application` / port=20128 / type=url / protocol=http / allUsers=true（原生 TCP 入口，gateway_socket 空）。