# fluxor（FnDepot fpk）

fork 自 [shuangji66/fluxor](https://github.com/shuangji66/fluxor)（Vue3 + Go 单体 mihomo 面板），
个人 fork：[Mike-hd123/fluxor](https://github.com/Mike-hd123/fluxor)。
本目录是 fnOS 原生 fpk 打包工程，版本 `1.4.0-6`（上游 `82686d9` / 1.4.0，本地号 `-6`）。

## 这个 fork 改了什么（vs 上游 82686d9）

### 后端

1. **默认捆绑 vernesong smart 内核**：`app/fluxor` = Go 后端 + `app/mihomo` = `mihomo-linux-amd64-alpha-smart-4bc3d49`（vernesong/mihomo Prerelease-Alpha），随包 `app/Model.bin`（LightGBM 选路模型），首启零下载即用 ML 选路。
2. **地理/ASN 三件套随包分发**：`GeoSite.dat` / `GeoIP.dat` / `ASN.mmdb` 全部打进 `app/`，install_callback 不存在才拷入 `shares/`，无网首启内核秒加载不下载 geo 库。
3. **配置模板 smart 化**：`groups_full.go` 与 `groups_base.go` 五个地区组（🇭🇰🇹🇼🇯🇵🇸🇬🇺🇸）均为 `type: smart`（uselightgbm + collectdata + include-all + sample-rate + prefer-asn）。
4. **smart 升级通道 + 降级闸门**：`POST /core/smart-update` 从 vernesong/mihomo 拉 smart 内核；`POST /upgrade` 先判 `IsCoreRunning` 三路分诊——未运行 503「请先启动内核」、PID 在位但 socket 不通 503「内核可能在重启中」、内核回 `already using latest` 识别为 200 no-op（`updated:false`），不再把「无更新」渲染成升级失败挂死按钮；内核探测瞬时失败 fail-closed 不放行通用升级。
5. **内核冷启动「假就绪」修复**：新增 `waitCoreSocketReady`（100ms 探测、8s 上限），`StartCore` 在进程存活后同步等待 `external-controller-unix` 的 `core.sock` 真正可拨通才返回；此前 exec 完成即报成功，前端紧接着拨不存在的 socket 表现为「请求内核失败」。超时不算失败（SSE 会自愈）。
6. **TProxy 冷启动三分支策略**：`ResetOnStartup` 不再无条件清零——磁盘无 `tproxy_enabled` 键（首次安装）默认开启并持久化、键为 true（重启）重装 nft/策略路由规则、键为 false 保持关闭，端口非法时 fail-safe 回退为关并打明确日志。
7. **TProxy 隧道设备防自锁**：`tunnelIfacePrefixes` 识别 `tun*/wg*/Meta*/ipvl*` 四类（覆盖 easytier、WireGuard、mihomo 自身 tun、fnOS 虚拟网口 `ipvl0@enp2s0`），`EnableTProxyRules` 按活跃隧道接口逐设备注入 `ip rule add oifname`，防穿透链路被 TProxy mark 劫持回环断链，`DisableTProxyRules` 对称清理；`parseTunnelInterfaces` 纯函数便于单测。
8. **TUN 与 TProxy 后端互斥兜底**：新增 `IsTUNEnabled` 读 `tun.enable`，落 nft 规则前离规则最近处再校验一次——前端 Config.vue 的互斥只拦 UI 路径，cron 脚本 / 手改 `fluxor.conf.json` / 第三方 OpenAPI 客户端直连后端时两套出口重定向不会叠加致整站打不开；读不到配置文件一律按 TUN 未启用放行（保守方向：宁可放行也不能悄悄关掉用户刚点的开关）。
9. **DNS 重定向端口可配**：从硬编码 1053 提升为持久化配置项 `tproxy_dns_redirect_port`（默认仍 1053 保持兼容），新增 `GET/PUT /config/tproxy/dns-port`，TProxy 已启用时改完即重建规则立即生效。
10. **ASN 降级而非 fatal**：`ASN.mmdb` 缺失时 `prefer-asn` 走 yaml 节点改写降级为 false（而非文本替换，避免与相邻键误替换、避免把布尔写成字符串）并打醒目日志，smart 组仍能按延迟测速优选。
11. **config.yaml 空壳 bug 修复**：`POST /subscribe/generate` 不再把请求体当全量配置——新增 `mergeSubscribeConfig` 按字段增量合并（零值字段视为「本次未提交」沿用内存态，仅列表型 `Subscriptions` 全量替换），新增 `SubscribeConfig.WithDefaults()` 退回出厂默认 7890/9090/7898 + base 规则集杜绝端口全 0；`active_subscription` 为空但订阅列表非空时默认选用首个订阅，覆盖「装完不点保存就重启」场景。
12. **fnOS 网关前缀双兼容**（4 文件）：
    - `gwprefix.go` — 剥前缀中间件（`/app/fluxor/xxx` → `/xxx`）
    - `web/index.go` — 检测网关请求注入 `<base href="/app/fluxor/">`
    - `main.go` — ctx 注入 `BASE_URL`，socket 迁 `@appcenter`，上游 `PublishCoreState` 合并保留
    - 前端源码 **0 改动**（用户诉求保留上游方案）
13. **fake-ip 网段避让**：默认 198.18.0.1/16 → `198.19.0.1/16`——198.18/16 是 mihomo 出厂默认、全网路由器大量占用易撞车，198.19/16 与 TR3000 侧既有全屋分流路由对齐，且不与 easytier(192.168.3.0/24) / 上级路由(192.168.19.0/16) / TProxy 私网 bypass 集冲突。

### 前端

- `Config.vue`：smart 内核升级入口（`isSmartCore` + `handleUpgradeCore` + 菜单分支）；TUN/TProxy UI 互斥。
- `i18n.ts`：smart 升级 + 内核失败可恢复提示文案（4 个冲突文件之一，已与上游 yaml.Node 重构合并）。
- `fetchWithRetry`：仅网络层失败退避重试、HTTP 4xx/5xx 不重试。
- `requestCore`：内核请求快重试通道（400ms 退避一次、总预算 <5s 避开网关 504 切断墙），配合内核重启窗口不再无限轮询挂死。

### 打包层

- **非 root 运行**：`config/privilege` `run-as: user`，systemd `User=nobody`
- **安装位置**：manifest 无 `install_type=root`，跟随用户选的空间（非系统分区）
- **数据目录**：`@appdata/<vol>/fluxor/shares/`
- 应用名大写 `Fluxor`（manifest display_name + ui/config title + systemd Description）
- fnOS 统一网关：socket `@appcenter/fluxor/app.sock`，gatewayPrefix `/app/fluxor`
- 卸载向导「保留/清除数据」选项（`wizard/uninstall` + `uninstall_callback` 读 `wizard_delete_data`）

## 端口对照

| 服务 | 端口 |
|---|---|
| 面板 Web（内网直连） | 18099 |
| 面板 Web（网关） | /app/fluxor |
| HTTP 代理 | 7890 |
| 混合/Tproxy | 7898 |
| API controller | 9090 |
| DNS | 1053（可配 `tproxy_dns_redirect_port`） |

> ⚠️ 7890/9090/7898/1053 与 ClashLite 同值，**面板端口 18099 可并存，代理层需二选一**——同机同时跑两个透明代理会叠加 nft 规则。

## 部署形态

- 原生 systemd 服务 `fluxor.service`（User=nobody），env 文件 `@appdata/fluxor/fluxor.env` 注入全部路径。
- 数据持久在 `@appdata/<vol>/fluxor/shares/`（fluxor.json / fluxor.conf.json / config.yaml / Model.bin / GeoSite.dat / GeoIP.dat / ASN.mmdb / proxies/），升级不覆盖。
- 内核升级旧二进制备份在 `@appcenter/fluxor/fluxor-backup/mihomo.alpha-smart.<时间戳>`，可手动回滚。
- **防回退约定**：fluxor 只操作 nft / 策略路由层面的透明代理，绝不设置系统级代理（`http_proxy`/`https_proxy` 环境变量、gsettings、NetworkManager、uci 等）。新增代码不得引入任何形式的系统代理写操作。

## 构建

```bash
# 1. 构建二进制（前端 dist 已 embed）
cd /vol2/1000/workspace/fluxor-work/fluxor/backend
go build -o ../fluxor .
# 2. 拷新产物进本工程
cp /vol2/1000/workspace/fluxor-work/fluxor/fluxor app/fluxor
# 3. 打包（产物落 /vol2/1000/download，不自动安装）
./build.sh
```

## 上游同步

上游 `shuangji66/fluxor` 有新 commit 时：`git fetch upstream && git merge upstream/main` → 重放本地 smart 补丁 → 重新构建打包（版本号 `<上游>-<本地>` 规则，本地号重置）。

## 版本史

| 版本 | 要点 |
|---|---|
| `1.4.0-6` | 内核冷启动假就绪修复 + 升级通道三路分诊 + TProxy 隧道防自锁 + TUN 互斥兜底 + DNS 端口可配 + ASN 降级 + 前端请求重试 |
| `1.4.0-5` | TProxy P0/P1 全量修复 + TUN 互斥兜底 + DNS redirect 端口可配 + ASN 降级 + configgen smart 组补齐 + subscribe/generate active_subscription 变更触发节点合入 |
| `1.4.0-4` | 修复 Smart 内核启动 fatal 崩溃（缺 ASN.mmdb）+ 应用名大写 Fluxor |
| `1.4.0-3` | ASN.mmdb 随包分发 + 稳定性四连修（内核请求容忍 / TProxy 冷启动三分支 / fake-ip 198.19/16 / 隧道设备 bypass） |
| `1.4.0-2` | generate 后 config.yaml 空壳 bug 修复 + active_subscription 兜底 |
| `1.4.0-1` | 同步上游 1.4.0（82686d9，10 commit / 92 文件）+ base 规则集补齐 5 个 smart 地区组 |
| `1.3.13-3` | 卸载保留/清除数据选项 + 非 root 运行 + 数据目录迁 shares + 端口回上游原值 + geo 库随包 |
| `1.3.13-2` | 桌面入口改走 fnOS 统一网关（app.sock 迁 @appcenter）+ 应用名简化为 fluxor |
| `1.3.13-1` | fork 首版：smart 内核 + smart 地区组 + 版本比较修复 + smart 升级通道 |
