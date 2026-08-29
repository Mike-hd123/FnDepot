# Quota 窗口展示修正设计

日期:2026-08-09

## 背景与动机

当前应用里 `codex` free 账号的 `quota_json` 实测为:

```json
{
  "version": 1, "exceeded": false, "reason": null, "next_recover_at": null,
  "limits": [
    { "limit_id": "codex", "limit_name": null,
      "primary":   { "used_percent": 0.0, "window_minutes": 43200, "reset_at": "2026-09-08T15:03:25+00:00" },
      "secondary": { "used_percent": 0.0, "window_minutes": 0,     "reset_at": null },
      "credits": null },
    { "limit_id": "codex-credits-has", "primary": null, "secondary": null, "credits": null }
  ]
}
```

暴露两个展示问题:

1. **标签误标**:`window_minutes=43200`(30 天 = 月限额)被 `QuotaBlock.tsx` 的 `>= 10080` 规则标成"周窗口"。free 号实际是**月限额**,非 free 才是周限额。
2. **空窗口噪音**:`secondary` 只有 `used_percent=0`(无 window_minutes、无 reset_at),被渲染成"次窗口"空条;`codex-credits-has` 全 null,也应丢弃。

用户确认:codex 无 5 小时限额;free = 月限额,非 free = 周限额。**上游返回什么就展示什么,没返回就不展示。**

## 决策

### D1:解析层按上游 `has_data` 规则丢弃空窗口 / 空限额项(Rust)

镜像上游 codex `rate_limits.rs` 的保留规则 —— 窗口只要满足以下任一即保留:

- `used_percent` 存在且非 0,或
- `window_minutes` 存在且非 0,或
- `reset_at` 存在

三者全空/0/缺失 → 丢弃窗口。限额项 `primary`/`secondary`/`credits` 全 `None` → 丢弃整个限额项。

落点:
- `codex_backend.rs::quota_window` → 无数据返回 `None`
- `codex_backend.rs::parse_limits` → 过滤全空限额项

行为影响:
- free 号 `secondary`(仅 used-percent=0)→ 丢弃;`codex-credits-has` → 丢弃;`primary`(window_minutes=43200 + reset_at)→ 保留
- `quota_from_headers` 的 `limits.is_empty() && status != 429 → None` 逻辑不变,空限额时不再写入 quota_json(保留 null/旧值)
- 路由层只消费 `exceeded`/`next_recover_at`,不读窗口,不受影响

### D2:标签只支持三种(TSX,±5% 容差),重置显示具体时间点

`QuotaBlock.tsx::windowLabel` 按 `window_minutes`(**单位是分钟**)近似匹配三种标签:

| `window_minutes` | 标签 |
|---|---|
| 300 (5h) | 5H限额 |
| 10080 (7d) | 周限额 |
| 43200 (30d) | 月限额 |
| 其它 | 裸「限额」(不猜时长) |

`window_minutes` 缺失 → 裸「限额」。**不硬编码 free/非 free** —— 上游返回 43200 就显示月、返回 10080 就显示周。

**重置显示具体时间点**:`resetLabel` 用 `reset_at` 本地化格式化为月/日/时/分(如 `9/8 15:03`),不用相对时长(曾误显示「30 天后」且标签「12小时窗口」因单位 bug 错乱)。`reset_at` 缺失 → **不渲染重置行**(用户要求:缺失就不显示重置时间点)。

### D3:渲染层防御性跳过空窗口(TSX)

`QuotaBlock.tsx` 渲染时用与 D1 相同的规则过滤窗口,使现存 DB 里的脏 `secondary` 立即不显示,不必等下次上游响应刷新。

### D4:reset_at 按可选字段处理

`reset_at` 为 `Option`,上游返回则展示具体时间点,缺失则不渲染重置行。plus 号实测(通过 `wham/usage`)返回 `reset_at`,正常显示。

### D5:主动探测专门限额端点(无流量时更新)

上游提供专门限额端点 `GET {backend-api}/wham/usage`(权威状态),实测:
- free 号:`limit_window_seconds=2592000`(30 天月限额)
- plus 号:`limit_window_seconds=604800`(7 天周限额),`used_percent=58`,`credits.balance="1908.09"`

`codex_backend.rs` 加 `quota_from_usage_payload`(归一化秒→分钟、UNIX→RFC3339,只保留 primary)+ `CodexProvider.fetch_quota`(GET `/wham/usage`)。`AuthService.sync_quota` 在**模型同步后/刷新后/维护循环(12h)**调用,失败静默保留旧值。`Provider` trait 加 `fetch_quota` 默认 no-op。

### D6:文档同步(`docs/auth-codex`)

- `00-facts.md`:去掉 "primary=5h / secondary=周" 硬编码,改为按实际返回 + 专门端点
- `work/02-design.md` §5.3:补充空窗口丢弃 + 动态标签 + 主动探测
- `01-ui-spec.md` 限额块示例:改用真实月窗口数据

## 测试

Rust `codex_backend.rs` 增加 table 测试:
- 仅 `used-percent=0` 的窗口被丢弃
- `window_minutes=43200` 保留(月窗口)
- `used-percent=23` 无时长保留(现有 `other` 用例不受影响)
- 全 null 限额项被丢弃
- `quota_from_usage_payload`:plus 周窗口(604800s→10080min)、free 月窗口(2592000s→43200min)、limit_reached→exceeded、缺 quota 数据→None
- `fetch_quota` 命中 `/wham/usage` 端点

`service.rs`:`sync_quota` 成功持久化、失败静默保留旧值。

前端无测试框架,不改。

## 不在范围

- 不写死 free/非 free 的窗口类型
- 不改路由层 quota 语义(路由只看 `exceeded`/`next_recover_at`)
