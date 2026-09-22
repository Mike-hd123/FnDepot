# ezbookkeeping (EZ记账) — Agent 须知

> 项目说明见 [README.md](./README.md)。本文件只讲 agent 该做什么 / 红线 / 坑。

## 定位
上游 [mayswind/ezbookkeeping](https://github.com/mayswind/ezbookkeeping) 的 fnOS 发行：**家庭记账**，本机唯一账本事实源（minibill 已退役）。当前发布 **2.0.0-1**。

## 架构
- `src/` — 上游源码 fork + fnOS 打包层（**manifest/build.sh/cmd 都在 src/ 里**，与其它应用目录布局不同）
- 本地优先 SQLite，多账本/预算/报表；支持微信/支付宝/信用卡账单导入，gzip 压缩提速，移动端触屏优化
- 2.x 增量：信用卡额度/可用额度环、洞察报表自定义图表、S3 对象存储、1.x→2.x 数据自动迁移无损
- 历史包 `ezbookkeeping-1.6.1-14-x86.fpk` 留档

## 坑
- **fnpack.json 索引滞后**：releases 还停在 1.6.1-12/14，2.0.0-1 未录入——发版收口必须同步索引（见根 AGENT.md 规矩）
- 记账口径/规则 → Hermes 技能 `ezbookkeeping-ops`（流水全在此）；账目核对走 Studio 工作流 f5dde188
- 数据在用户数据目录（@appdata），改动前备份

## 红线
同根 [AGENT.md](../AGENT.md)。
