# hyatlas.fpk — HyAtlas(混元记忆) 安装包

- 版本：2.0.1（内含 site-packages hyatlas_memory 3.5.0）
- 打包时间：2026-08-29
- 上游项目：[tuancookiez-hub/HyAtlas-Memory](https://github.com/tuancookiez-hub/HyAtlas-Memory)（v3.5.0）
- 源码：本目录 `src/`（上游源码；汉化/降权等改动在打包层，未侵入上游源码树）
- 打包方式：fnOS `fnpack build`

## 相对上游的核心改动（在 fpk 打包层）

1. **面板汉化**：site-patches/dashboard/（app.js、dashboard.html、l5.js、observatory.js、styles.css）中文界面。
2. **dash token 固定**：`_DASH_TOKEN_BASE` admin/home 双位兜底，token=admin 双向读写，无 cookie 自动 302 登录。
3. **zvec 双索引**：`MEMORY_VECTOR_STORE` 默认 zvec。
4. **降权运行**：systemd 双 unit（server+dashboard）以 `hy-memory` 用户运行，数据落 `/vol1/@appshare/hyatlas`。

## 安装

fnOS 应用中心添加外部源 `https://github.com/Mike-hd123/FnDepot` 后安装，或直接下载本 fpk 手动安装。注意安装包约 100MB，内含完整 Python site-packages 离线依赖。
