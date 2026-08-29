# 术语表（Glossary）

> grill-with-docs：随会话增长的术语表。事实条目与决策条目混排，标注来源。

| 术语 | 定义 | 来源 |
| --- | --- | --- |
| 渠道（Channel） | 上游供应商配置，网关的转发目标；`channels.api_key` 存上游厂商凭证 | 代码 |
| API 密钥（ApiKey） | 网关**发给下游客户端**的访问凭证，与上游渠道无关 | 代码 |
| AuthScheme | 上游凭据的放置方式枚举：Bearer / x-api-key / query / optional_bearer | 代码 |
| 上游密钥 vs 下游密钥 | 渠道里的 `api_key` 是上游厂商的；`api_keys` 表是网关自签发给客户端的 | 代码 |
| Auth 账号（Auth Account） | 用户在各厂商（ChatGPT/Claude/...）的登录账号，登录后授权 WaLiAPI 代表其访问厂商后端 | 本会话 |
| auth.json | Codex CLI 的登录令牌文件：`~/.codex/auth.json`（或 `$CODEX_HOME/auth.json`），含 `access_token` / `refresh_token` / `account_id` | 外部源 |
| PKCE | OAuth 授权码流加固（S256），Codex 默认使用 | 外部源 |
| backend-api | ChatGPT Web 后端（chatgpt.com/backend-api），Codex 用 ChatGPT 登录后走此路径 | 外部源 |
| 账号型上游（account-as-upstream） | 把用户订阅账号当作一个上游渠道，网关以 OAuth 令牌代表用户访问厂商后端，消耗订阅额度 | ADR-1 |
| auth_accounts | WaLiAPI 新增的账号表：通用列（provider / label / account_id / status / disabled / priority / weight / quota_json / model_states_json / attributes_json / last_refreshed_at / next_refresh_after / next_retry_after / created_at / updated_at）+ `payload_json` 存 provider 特有令牌载荷（codex 的 access_token / refresh_token / expires_at 等，不设独立令牌列） | ADR-3 |
| payload_json | auth_accounts 中存 provider 特有令牌载荷的 JSON 列（codex 的 token 字段等），使通用列不绑死 codex | ADR-3 |
| QuotaState | 账号限额运行时状态：Exceeded / Reason / NextRecoverAt / BackoffLevel（指数退避） | CPA 调研 |
| ModelState | 账号下某模型的执行状态：Status / Unavailable / NextRetryAfter / LastError / Quota | CPA 调研 |
| cooldown | 限额/错误触发后账号暂时退出路由，到期自动恢复；状态独立持久化 | CPA 调研 |
| provider 维度 | 账号来源维度：codex / claude / kiro / kimi；同 provider 可多账号并存 | ADR-5 |
