# wechat-on-cloud.fpk — 云微(飞牛云微信) 安装包

- 版本：1.5.1（面板自报 WOC_VERSION 同步对齐）
- 打包时间：2026-08-29
- 上游项目：[Gloridust/WechatOnCloud](https://github.com/Gloridust/WechatOnCloud)（fork 存档：[Mike-hd123/WechatOnCloud](https://github.com/Gloridust/WechatOnCloud)）
- 源码：本目录 `src/`（含全部 NAS 适配改动）
- 打包方式：fnOS `fnpack build`

## 相对上游的核心改动（全部已进 src/ 存档）

1. **ipvlan 单网卡**：删除 woc-net bridge 方案，实例容器只挂 `woc-lan`（ipvlan l2，192.168.5.0/24），面板经宿主 ipvl0 虚接口访问容器；`WOC_DOCKER_NETWORK=woc-lan` 写死默认值（cmd/main:100）。
2. **数据卷主机 bind**：`WOC_DATA_DIR` 非空时容器 /config 落主机路径 `/vol2/1000/weixin/woc-data-<id>`，否则回退 docker 命名卷。
3. **移动端触屏修复**：面板 web 不再自动抢焦点（加载/接管跳过 focusFrame，点画面才聚焦），bundle index-DViroXe_.js。
4. **版本号对齐**：WOC_VERSION 1.4.9 → 1.5.1。
5. fnOS 原生化：systemd 风格 cmd/ 脚本、wizard 安装向导、图标三路径、`PANEL_ALLOWED_HOSTS` host-guard。

## 安装

fnOS 应用中心添加外部源 `https://github.com/Mike-hd123/FnDepot` 后安装，或直接下载本 fpk 手动安装。
