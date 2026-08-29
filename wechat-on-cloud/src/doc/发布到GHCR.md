# 发布镜像

> 返回 [← README](../README.md)

把两个镜像（`woc-panel`、`wechat-on-cloud`）发布到容器仓库，供他人 `docker compose up -d` 直接拉取。默认走 GHCR；可选同步推到 Docker Hub。

---

## 方式 A · GitHub Actions（推荐）

仓库自带 GitHub Actions（[.github/workflows/release.yml](../.github/workflows/release.yml)），在你**推送 `vX.Y.Z` 标签或发布 Release** 时，自动构建多架构（amd64+arm64）镜像并推到 GHCR：

```bash
git tag v1.0.0
git push origin v1.0.0     # 触发 Actions，产出 ghcr.io/<owner>/woc-panel:1.0.0 等标签
# 或在 GitHub 上 Publish 一个 Release（会额外打 latest）：
gh release create v1.0.0 --title v1.0.0 --notes "..."
```

> 注意：单纯 push tag 只产出 `X.Y.Z / X.Y / X`，**不会更新 `latest`**；要更新 `latest` 请改用 **发布 Release** 或在 Actions 里手动 `workflow_dispatch`。

### 可选：同步推到 Docker Hub

GHCR 拉取在国际网络下偶尔被 TLS / DNS 干扰；Docker Hub 覆盖面更广，且公开镜像 `docker pull` 无需先 `docker login`。workflow 已内置 Docker Hub 双推开关：**配齐两个变量即可启用，没配就保持只推 GHCR**（向后兼容）。

**一次性配置**（GitHub repo → Settings → Secrets and variables → Actions）：

| 类型 | Name | Value |
|---|---|---|
| Variable | `DOCKERHUB_USERNAME` | 你的 Docker Hub 用户名（如 `gloridust`） |
| Secret | `DOCKERHUB_TOKEN` | Docker Hub Access Token（[hub.docker.com → Account Settings → Personal access tokens](https://hub.docker.com/settings/personal-access-tokens) → New Access Token，权限选 `Read & Write`） |

> Variable 和 Secret 是两个不同的 tab；用户名放 Variable 即可（不敏感），Token 必须放 Secret。
> 在 Docker Hub 上**预先建好两个 public repo**：`<用户名>/woc-panel`、`<用户名>/wechat-on-cloud`（hub.docker.com → Create Repository），否则首次推送会失败（Docker Hub 不会自动建 repo）。

配齐后，下次发版（或 `workflow_dispatch` 手动触发）就会同时推到：
- `ghcr.io/<github-owner>/woc-panel:X.Y.Z`
- `docker.io/<dockerhub-user>/woc-panel:X.Y.Z`

使用者在 `.env` 里 `WOC_IMAGE_PREFIX=docker.io/<dockerhub-user>` 即可从 Docker Hub 拉。

---

## 方式 B · 本机 buildx 手动构建并推送（不走 Actions）

适合想立刻出包、或不依赖 CI 的场景。需要 Docker Buildx（Docker Desktop 自带；纯 Linux 跨架构需先装 QEMU：`docker run --privileged --rm tonistiigi/binfmt --install all`）。

```bash
# 1) 登录 GHCR（PAT 需 write:packages 权限）
echo <YOUR_GITHUB_PAT> | docker login ghcr.io -u <github 用户名> --password-stdin

# 2) 首次创建并启用多架构构建器（已建过改用 docker buildx use woc）
docker buildx create --name woc --use

# 3) 构建并推送两个镜像（amd64 + arm64）。VER 与 git tag 保持一致（不带 v）
VER=1.0.1
docker buildx build --platform linux/amd64,linux/arm64 \
  -t ghcr.io/gloridust/woc-panel:$VER -t ghcr.io/gloridust/woc-panel:latest \
  --push ./panel
docker buildx build --platform linux/amd64,linux/arm64 \
  -t ghcr.io/gloridust/wechat-on-cloud:$VER -t ghcr.io/gloridust/wechat-on-cloud:latest \
  --push ./docker
```

> 把 `gloridust` 换成你的 GHCR 命名空间（与 `docker-compose.yml` / `WOC_IMAGE_PREFIX` 一致）。
> 只想本机自用、不推 GHCR，用 [`./scripts/build-local.sh`](../scripts/build-local.sh) 构建本机架构单架构镜像即可。

---

## 发布后：把包设为公开

首次发布后还需把 GHCR 包设为公开，否则别人 `docker compose up -d` 会报 `denied`：

1. 打开 GitHub → 你的头像 → **Packages** → 分别进入 `woc-panel`、`wechat-on-cloud`；
2. **Package settings → Change visibility → Public**。

> 若想保持私有，则使用者需先 `docker login ghcr.io`（用具备 `read:packages` 的 PAT）才能拉取。
> 在镜像发布之前，本地用 [`./scripts/build-local.sh`](../scripts/build-local.sh) 自构建即可，无需等待发布。

---

## Telegram 发布通知（可选，免服务器）

仓库内置 [.github/workflows/telegram-notify.yml](../.github/workflows/telegram-notify.yml) + [.github/scripts/tg-notify.mjs](../.github/scripts/tg-notify.mjs)：**新版本发布** / **新 issue** 时，把内容（GitHub Markdown 渲染为 Telegram HTML）推送到群组。跑在 GitHub Actions 上，无需服务器；未配置则自动跳过。**发布通知还会自动置顶，并取消上一个 release 的置顶**（群里始终只置顶最新版本）。

一次性配置：

1. 把机器人（如 `@WechatOnCloudBot`）拉进目标 Telegram 群组。**要让"自动置顶"生效，需把机器人设为群管理员并开启「置顶消息」权限**（缺权限时通知照发、仅置顶被跳过，不影响主流程）。
2. 取群组 chat id：bot 进群后在群里发条消息，浏览器打开 `https://api.telegram.org/bot<BOT_TOKEN>/getUpdates`，找 `result[].message.chat.id`（群组通常是 `-100` 开头的负数）。
3. 仓库 **Settings → Secrets and variables → Actions**：
   - **Variables** 标签 → `TELEGRAM_CHAT_ID` = 上面的 chat id；
   - **Secrets** 标签 → `TELEGRAM_BOT_TOKEN` = [@BotFather](https://t.me/BotFather) 给的 token。

之后每次「发布 Release / 新建 issue」都会自动推送。想关掉 issue 推送，删掉 workflow 里 `on:` 下的 `issues:` 即可。

> **自动置顶原理**：发完新消息后，用 `getChat` 读出群当前置顶消息（即上一个 release），置顶新消息、再取消旧的——无需任何持久化存储。issue 通知不置顶。手动 **Run workflow** 也会置顶（发的是最新 release），可用来测试。

---

## Telegram 命令机器人（可选，免服务器，轮询版）

除了发布通知，仓库还内置 [.github/workflows/telegram-bot.yml](../.github/workflows/telegram-bot.yml) + [.github/scripts/telegram-bot.mjs](../.github/scripts/telegram-bot.mjs)：让机器人在**私聊 / 群组**里响应命令，跑在 GitHub Actions cron 上，**不需要服务器**。

命令：`/help`、`/releases`、`/release <tag>`、`/issues`、`/issue <编号>`。

启用：

1. 复用上面的 `TELEGRAM_BOT_TOKEN`（Secret）；
2. 仓库 **Settings → Secrets and variables → Actions → Variables** 新建 `TELEGRAM_BOT_ENABLED = true`（不设为 `true` 则该工作流不运行）；
3. 把机器人拉进群组，或在私聊里 `/start`。

> **原理**：每 5 分钟（cron 最小间隔）调用 `getUpdates` 拉取待处理命令、回复、再用 `offset` 向 Telegram 确认（Telegram 自己保存 offset，**无需任何持久化存储**）。
> **局限**：命令**非实时**——受 cron 最小 5 分钟 + GitHub 排队延迟影响；且 GitHub 会在仓库 60 天无活动时暂停定时任务（去 Actions 页手动重启即可）。想立即处理一次：Actions → telegram-bot → **Run workflow**。要真正实时，得改用 webhook（需一个 serverless 端点）。
