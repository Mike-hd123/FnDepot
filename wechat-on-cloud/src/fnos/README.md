# 飞牛 fnOS 应用打包（.fpk）

把 WechatOnCloud 打成飞牛应用中心可安装的 `.fpk`。本目录 `woc/` 是一个按飞牛开发文档组织的 Docker 类应用工程，应用中心安装后会直接执行 `app/docker/docker-compose.yaml` 来启停应用。

> 飞牛开发文档：<https://developer.fnnas.com/docs/guide/>（fpk 结构、manifest、Docker 应用、向导等）

## 目录结构

```
woc/
├── app/docker/docker-compose.yaml  # 应用中心直接执行的 compose（= 仓库根 compose，挂 docker.sock）
├── cmd/main                        # 生命周期入口：start/stop 返回 0，status 看 woc-panel 容器状态
├── config/privilege               # 运行身份（run-as=package）
├── config/resource                # 默认共享目录
├── wizard/install                 # 安装向导：填管理员账号/密码、端口、镜像版本（字段名即环境变量名）
├── manifest                        # INI 元数据（appname/version/display_name/platform=all/service_port…）
├── ICON.PNG                        # 64×64
└── ICON_256.PNG                    # 256×256
```

## 构建

需先在开发机装飞牛官方 `fnpack` CLI（见上方文档）。

```bash
cd fnos/woc
fnpack build           # 产出 wechat-on-cloud-<version>.fpk
```

然后在飞牛「应用中心 → 手动安装」上传该 `.fpk`。安装向导会要求设置管理员密码与端口。

## ⚠️ 重要前提与待验证项

本工程依据公开开发文档编写，**尚未在真实 fnOS 设备上验证**，上架前请实测以下两点：

1. **docker.sock 权限（关键）**：面板需挂载宿主 `/var/run/docker.sock` 来按需创建/销毁微信实例容器，这等同宿主 root 权限。`config/privilege` 当前设为 `run-as=package`（普通应用用户）；若该用户无权访问 docker.sock，新建实例会失败。届时可能需要把应用用户加入 `docker` 组，或申请飞牛 `run-as=root` 合作权限（官方文档注明 root 需官方合作）。
2. **向导环境变量注入 compose**：`wizard/install` 的字段名（`WOC_USER`/`WOC_PASSWORD`/`WOC_HTTP_PORT`/`WOC_VERSION`）会成为环境变量，compose 里用 `${...}` 取用。请确认应用中心执行 compose 时这些变量确实可见（不同 fnOS 版本行为可能不同）。

镜像本身从 GHCR 拉取（多架构 amd64/arm64），与仓库根 `docker-compose.yml` 完全一致，故 fpk 内不含镜像、体积很小。
