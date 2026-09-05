# octopus.fpk — Octopus 安装包

- 版本：0.13.2-5
- 打包时间：2026-09-05
- 上游项目：[bestruirui/octopus](https://github.com/bestruirui/octopus)
- 源码：本目录 `src/`（上游 v0.13.2 tag 原样）+ `src/fnos/`（fnOS 打包层：cmd/manifest/config/gateway）
- 打包方式：fnOS `fnpack build`

## 打包说明

1. 官方 0.13.2 linux-amd64 release 二进制（约 51MB，前端已嵌入），未修改。
2. fnOS 原生 volume 安装（落 `/vol1/@appcenter/octopus/`）：cmd/ 生命周期脚本 + install_callback 数据目录准备，`OCTOPUS_SERVER_PORT=8081`（避开云微 8080）。
3. 数据目录 `/vol1/@appdata/octopus/data.db`（TRIM_PKGVAR 注入，卸载重装不丢），SQLite 存储。
4. **v3-v4**：接入飞牛统一网关 socket 自注册（对齐 minibill），gateway sidecar 监听 `APPDEST/app.sock` → TCP 8081，手机端 /app/octopus 恢复路由。
5. **v5（2026-09-05）**：gateway sidecar 由裸 TCP 盲转发升级为**剥前缀 HTTP 反代**（修复手机端 404），详见 `/vol2/1000/download/octopus-0.13.2-5-x86-修复说明.md`。

## src/fnos/ 重组装

```bash
# 结构：cmd/ config/ gateway/ manifest + ICON* + wizard/install + app/(二进制+ui)
cp -a src/fnos/cmd   <BUILD>/cmd
cp -a src/fnos/config <BUILD>/config
cp -a src/fnos/gateway <BUILD>/app/gateway   # app/ = octopus 二进制 + ui/
cp src/fnos/manifest <BUILD>/manifest
fnpack build -d <BUILD>
```

## 安装

fnOS 应用中心添加外部源 `https://github.com/Mike-hd123/FnDepot` 后安装，或直接下载本 fpk 手动安装。
