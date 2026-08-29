# T07：草稿连通性与端点测试

## 目标

实现保存前不落库的 `test_channel_draft`，按用户勾选的实际端点逐一验证；测试失败给出分类和修复建议，同时允许用户明确忽略测试结果保存。

## 依赖

- T01 preset 测试策略。
- T02 输入 DTO 与 URL 双写规则。
- T06 endpoint executors。

## 文件所有权

- 修改 `src-tauri/src/commands/channel.rs`，新增 draft test command。
- 可新增 `src-tauri/src/services/channel_test.rs`。
- 修改 `src/lib/api.ts`、`src/types/index.ts` 增加测试 DTO。
- 不修改 ChannelForm 交互；T08 消费该 API。

## API 契约

输入是完整但未保存的渠道草稿，包括 protocol、provider、native base、legacy base 派生所需数据、API Key、模型、native endpoints、timeout。编辑场景 API Key 留空时，命令可通过 channel id 在后端读取现有 Key，但不得返回给前端。

每个端点返回：endpoint、passed/failed/skipped、failure category、脱敏 message、latency、tested model、是否可能产生费用。整次返回另含 `draft_fingerprint`、`tested_at` 与稳定的 `test_run_id`；fingerprint 覆盖 protocol、provider、规范 URL、模型、端点、timeout 和 API Key 的不可逆后端指纹，不包含或泄漏明文 Key。

failure category 至少包括 network、timeout、authentication、endpoint_unsupported、model、request、protocol、unknown。

## 测试策略

- OpenAI Chat：实际最小非流推理请求。
- OpenAI Responses：实际最小非流 Responses 请求；不能用 `/models` 代替。
- 两者都勾选时分别测试，结果独立。
- Anthropic Messages：实际最小非流 Messages 请求。
- Anthropic count_tokens：只有能力存在时单独测试。
- Ollama `/api/chat`：实际最小请求；空 Key 合法。
- Embeddings：模板声明且用户配置该能力时用最小 input。
- 模型枚举可做基础连通补充，但不能证明推理端点可用。

测试请求使用首个模型、最小输出、stream=false。界面必须说明可能产生极少上游费用。草稿测试不进入生产 quota、不写生产 request log；仅允许写脱敏诊断日志。

## 保存决策

- 所有已选端点通过：允许正常保存。
- 任一失败或 skipped：返回完整结果，由 UI 默认提供“修改配置”，次级操作“仍然保存”。
- Chat 通过、Responses 404/501：明确建议取消 Responses 勾选并重试。
- 用户强制保存时不篡改端点配置，渠道状态显示最近测试失败/未验证。
- OpenAI 没勾选任何 Chat/Responses：本地校验直接拒绝，不发测试。
- 测试后只要 protocol、provider、URL、Key、模型、端点或 timeout 改变，原结果立即失效，保存必须重新测试；名称、备注、priority、weight 的变化不使连通性结果失效。
- 新版 UI 保存携带 `test_run_id + draft_fingerprint + force_save`。后端校验它们与当前草稿一致；普通保存要求所有选中端点通过，强制保存只要求同一草稿已完成一次测试。旧前端 payload 不含这些字段时继续按旧兼容路径处理，避免破坏旧版本。
- 强制保存成功后，将同一次测试的时间和聚合结果写入渠道现有 `last_test_at/last_test_ok`；草稿测试本身仍不提前写 channels 或生产 request log。

## 安全要求

- API Key 不出现在返回、错误文本或日志。
- 上游 response body 仅截取脱敏诊断摘要。
- 草稿 URL 必须通过 http/https、host 和 SSRF 策略；localhost 对 Ollama/custom 是合法例外，其他私网访问规则明确配置。
- 测试 timeout 独立于生产渠道 timeout，并有总上限。

## 实施步骤

1. 定义输入和逐端点结果 DTO。
2. 根据 preset/custom 构建不落库 identity。
3. 复用 T06 executor，禁止另写一套 URL 拼接。
4. 实现端点最小请求与错误分类。
5. 处理编辑留空 Key 和 Ollama 显式空 Key。
6. 注册 Tauri command 和前端 API。
7. 用 mock upstream 覆盖双端点、认证、超时、不支持和强制保存所需结果。
8. 实现短时、进程内 test-run store 或等价签名 receipt，校验 fingerprint、过期时间、单次草稿一致性；测试进程重启后 receipt 失效并要求重测。

## 验收标准

- OpenAI 双选必定产生两个独立测试结果。
- Responses 未提供时有明确提示，不把 Chat 成功当 Responses 成功。
- 测试失败不会写入 channels 表。
- API Key 和请求 secret 不泄漏。
- UI 可以依据稳定分类实现修改/强制保存流程。
- 修改连接相关字段后旧 test run 不能用于保存；强制保存可审计且不绕过“至少测试一次”。

## 测试命令

- `cargo test channel_draft_test --manifest-path src-tauri/Cargo.toml`
- `pnpm build`

## 交接输出

提供 API JSON 示例、endpoint 测试请求表、费用提示文本、failure category 表与 mock 结果。
