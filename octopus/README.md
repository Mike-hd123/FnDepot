# octopus.fpk — Octopus 安装包

- 版本：0.13.4-1（2026-09-10）
- 打包时间：2026-09-10
- 上游项目：[bestruirui/octopus](https://github.com/bestruirui/octopus)
- 源码：本目录 `src/`（上游 v0.13.4 release 二进制，前端已嵌入）+ `src/fnos/`（fnOS 打包层：cmd/manifest/config/gateway）
- 打包方式：fnOS `fnpack build`

## 打包说明

1. 官方 0.13.4 linux-amd64 release 二进制（约 51MB，前端已嵌入），未修改。
   zip 内附带的 LICENSE / README.md / THIRD_PARTY_LICENSES.csv 一并同步替换。
2. fnOS 原生 volume 安装（落 `/vol1/@appcenter/octopus/`）：cmd/ 生命周期脚本 + install_callback 数据目录准备，`OCTOPUS_SERVER_PORT=8081`（避开云微 8080）。
3. 数据目录 `/vol1/@appdata/octopus/data.db`（TRIM_PKGVAR 注入，卸载重装不丢），SQLite 存储。
4. **v3-v4**：接入飞牛统一网关 socket 自注册（对齐 minibill），gateway sidecar 监听 `APPDEST/app.sock` → TCP 8081，手机端 /app/octopus 恢复路由。
5. **v5**：gateway sidecar 由裸 TCP 盲转发升级为**剥前缀 HTTP 反代**（修复手机端 404）。
6. **v6-v8**：sidecar 迭代修 SPA 白屏——SSE 流式透传、POST Content-Length 透传、JS chunk 同版本参数注入保证单模块图单实例（ThemeProvider Context 不断裂）。
7. **v1/0.13.4-1（2026-09-10）**：同步上游 v0.13.4（请求头新增 client_header、渠道禁用 disable channel、前端日志 UI 优化），保留本地 gateway sidecar v8 补丁。上游大版本升级，本地补丁版本号重置为 1。
   （注：0.13.3-1 一轮未单独提交，与本轮合并为单次版本提交。）

## src/fnos/ 重组装

```bash
# 结构：cmd/ config/ + app/(二进制+ui+gateway) + manifest + wizard + ICON*
cp -a src/fnos/cmd      <BUILD>/cmd
cp -a src/fnos/config   <BUILD>/config
cp src/fnos/manifest    <BUILD>/manifest
# app/ 内容 = octopus 二进制 + ui/ + gateway/gateway_proxy.py + LICENSE/README/THIRD_PARTY_LICENSES
# wizard/ + ICON* 复用当前 fpk 内文件（不在 src/）
fnpack build -d <BUILD>   # fnpack 自动把 app/ 压成 app.tgz
```

## 安装

fnOS 应用中心添加外部源 `https://github.com/Mike-hd123/FnDepot` 后安装，或直接下载本 fpk 手动安装。