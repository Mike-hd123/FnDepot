# wechat-on-cloud (云微) — Agent 须知

> 项目说明见 [README.md](./README.md)。本文件只讲 agent 该做什么 / 红线 / 坑。

## 定位
上游 [Gloridust/WechatOnCloud](https://github.com/Gloridust/WechatOnCloud) 的 fnOS 原生发行 fork：NAS 上跑微信的 Web 面板（Node.js + dockerode 管理微信实例容器）。当前发布 **1.4.9-3**，面板端口 8080。

## 架构
- `src/` — 上游源码 fork + 全部 NAS 适配改动（Node.js 面板）
- 打包层：`cmd/`（生命周期脚本）、`wizard/`、manifest 在 src 内；`fnpack build` 产 `wechat-on-cloud.fpk`
- 实例容器网络：ipvlan **woc-lan**（l2，192.168.5.0/24）单网卡直连局域网，面板经宿主 `ipvl0` 虚接口访问

## 本地核心改动（相对上游，勿在合并时丢失）
1. ipvlan 单网卡（删了上游 woc-net bridge 方案），`WOC_DOCKER_NETWORK=woc-lan` 写死默认
2. 数据卷主机 bind：`WOC_DATA_DIR` 非空 → 容器 /config 落 `/vol2/1000/weixin/woc-data-<id>`
3. 移动端触屏：面板不自动抢焦点（跳过 focusFrame，点画面才聚焦）
4. 「电源」下拉：重启（重建容器保数据）+ 关机（stop 实例）
5. `PANEL_ALLOWED_HOSTS` host-guard；图标三路径；wizard 安装向导
6. WOC_VERSION 固定跟上游 tag（本地改动不动版本号）

## 坑
- 与 Octopus(8081)/Vikunja(3456) 端口错开，8080 归它
- 微信容器"版本过低"类问题 → Hermes 技能 `wechat-selkies-ops` / `wxbackup-data-access`
- 聊天记录备份走 WxBackup（.xb=SQLite3+XOR 0x36），别动本应用数据目录

## 红线
同根 [AGENT.md](../AGENT.md)：改动只落 src/，版本号 `上游-本地`，合并上游必做定制存活核对。
