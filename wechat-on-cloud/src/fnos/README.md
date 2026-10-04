# 飞牛 fnOS 打包层（fnos-native/）

本项目 fpk 打包层已还原至 `src/fnos-native/`。旧时代残留工程 `woc/`、`woc-fixed/`（docker-compose 型打包实验）已于 2026-08-30 删除，仅作历史参考可在 git 历史中查看。

## 目录结构

```
src/fnos-native/
├── app.tgz            # 应用包定档（字节原样取自已发布 fpk，不重打；含 server/ui/web-dist/config）
├── cmd/               # 生命周期脚本（main / *_callback / *_init），install_callback 需 root（建 woc-lan ipvlan + systemd 固化 ipvl0）
├── config/privilege   # 运行身份：run-as=root（install_callback 的 docker/systemd 操作 package 用户无法执行）
├── config/resource    # 资源声明 {}
├── wizard/install     # 安装向导（PORT / WOC_VERSION=1.5.1 / WOC_WECHAT_IMAGE）
├── manifest           # INI 元数据；checksum 字段 = md5(app.tgz)
├── build-app.sh       # 从 panel 源码全量构建 app.new.tgz（npm ci + vite build）
└── build.sh           # 一键构建脚本（默认定档 app.tgz；--from-source 用 app.new.tgz）
```

## 构建

```bash
# 上游同步后的标准链路：源码全量构建 → from-source 拼 fpk
bash src/fnos-native/build-app.sh
bash src/fnos-native/build.sh --from-source
```

产物覆盖项目根 `wechat-on-cloud.fpk`。定档路径（不带 `--from-source`）保留作回归：直接取 `app.tgz` 原样拼包。

## checksum 约定

- `manifest` 的 `checksum` 字段 = `md5(app.tgz)`，定档当前值为 `de58c35878bbd92aa2b4ea383973e14e`。
- `app.tgz` 定档自已发布 fpk 原包（面板源码在 `src/panel` 可复现，但构建产物带 node_modules、环境差异导致字节不稳定，故定档）。更新 `app.tgz` 时必须同步重算并更新 `manifest` 的 `checksum` 行，`build.sh` 会校验两者一致否则中止。
- `build.sh` 内部先跑 `fnpack build -d .` 做结构校验，再用 python3 tarfile 确定性重拼（fnpack v1.2.4 会重压 app.tgz 并改写 checksum，其产物不直接发布），保证 `tar tzf` 清单与 manifest 与原包逐行一致。
