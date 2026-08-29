# T02：数据库迁移与身份兼容

## 目标

增加渠道协议身份字段，提供唯一 `resolve_channel_identity()`，确保旧数据库、旧前端 payload、新配置在旧二进制中的降级调用、回滚旧版写入后再次升级全部可用且不丢字段。

## 依赖

- T00。
- T01 的领域枚举与 preset lookup 接口；若并行开发，先冻结相同枚举字符串。

## 文件所有权

- 新增 `src-tauri/migrations/015_channel_protocol_identity.sql`，编号如已有占用则顺延。
- 修改 `src-tauri/src/db/models.rs`、`src-tauri/src/db/repository.rs`、`src-tauri/src/db/mod.rs`。
- 新增/修改 `src-tauri/src/core/channel_identity.rs`。
- 修改 `src-tauri/src/commands/channel.rs` 的存储/输入/输出 DTO。
- 可新增 migration/identity 集成测试文件。
- 不修改 handlers、dispatcher、codec、React UI。

## Schema

向 `channels` 增加：

- `protocol TEXT NULL`
- `provider TEXT NULL`
- `native_base_url TEXT NULL`
- `native_endpoints TEXT NOT NULL DEFAULT '[]'`
- `preset_revision TEXT NULL`
- `identity_revision INTEGER NOT NULL DEFAULT 0`
- `legacy_executor_override TEXT NULL`

旧 `type/base_url` 保留并继续双写。迁移不改 API key、模型、映射、优先级、权重、状态、timeout、config 和时间字段。

## 身份解析规则

`resolve_channel_identity(row)` 是 DTO、路由、测试、导入导出的唯一入口。以下情况视为 legacy-uninitialized：`identity_revision=0`、protocol/provider 为空、native endpoints 为空且无法由 preset 明确解释。

- `openai` → OpenAI 协议、原生 Chat、`responses_via_chat_v1`；历史 URL 与 OpenAI 官方 canonical host 匹配时 provider 为 OpenAI，否则 provider 为 custom，避免把私有网关误标为官方供应商。
- `deepseek` → OpenAI/DeepSeek、原生 Chat。
- `claude` → Anthropic 协议、原生 Messages；仅官方 canonical host 标 provider=Anthropic，命中内置兼容预设时标对应 provider，其余为 custom；不无条件推断 count_tokens。
- `gemini` → UI 身份 OpenAI/Google，但保留 `legacy_executor_override=gemini_native`、原 URL 与 query-key 鉴权。
- `qwen/zhipu/moonshot/doubao` → OpenAI/对应提供商、原生 Chat。
- `ollama` → Ollama/Ollama、原生 api_chat；只剥除 Base URL 精确末尾 `/v1` 生成 native base。
- `custom/未知` → OpenAI/custom、原生 Chat 与 `responses_via_chat_v1`，因为当前 fallback adaptor 明确使用 `/chat/completions`；保留原 URL，不伪造具体厂商。

## 新配置向旧代码降级

- OpenAI 新渠道双写旧 `type=openai` 和旧版可调用的 OpenAI 兼容 `base_url`。
- Anthropic 新渠道双写 `type=claude`；`base_url` 必须是旧 Claude 适配器追加 `/messages` 后得到正确最终 URL 的兼容根，`native_base_url` 保存 UI 规范根。
- Ollama 原生新渠道双写 `type=openai` 与 `base_url=http://localhost:11434/v1`，`native_base_url=http://localhost:11434`。
- Google OpenAI 兼容新渠道写 `type=openai`，避免旧 Gemini 原生适配器错误接管。

每个 preset 必须有 mock URL 测试，断言新代码和上一版代码最终请求 URL 均正确。

## 回滚后再升级

固定实现 SQLite AFTER UPDATE trigger：当旧身份字段 `type/base_url/config` 的值发生变化时，清空 `protocol/provider/native_base_url/native_endpoints/preset_revision/legacy_executor_override` 并将 `identity_revision` 置 0。新版 update 必须在一个事务内执行两步：先更新旧业务字段并让 trigger 失效身份，再用最后一条 UPDATE 写入完整新身份和当前 revision；不得用单条 UPDATE 同时写新旧字段。新版 create 可一次 INSERT 完整字段，旧版 INSERT 依靠默认 revision 0。

旧版 INSERT 默认 revision 0；新版读取时实时解析，不依赖 migration 再运行。

## DTO 契约

- Storage model 允许 nullable legacy 字段。
- Create input 的新字段全部 `Option + serde(default)`；缺失时按旧 payload 推断。
- Update input 的 `None` 表示保持原值；显式空 native endpoints 拒绝。
- Output DTO 始终返回规范化 protocol/provider/native URL/endpoints/revision，并补齐现有遗漏的 `timeout_secs`。
- API Key 返回仍保持脱敏；编辑留空不修改与 Ollama 显式清空 Key 必须通过独立 patch 语义区分。

## 实施步骤

1. 添加 migration 和 trigger，验证全新数据库与升级数据库。
2. 扩展 storage model，避免 `SELECT *` 列对齐问题。
3. 实现 identity resolver、legacy inference、new-to-legacy writer。
4. 将 create/update 包装为事务，双写新旧身份。
5. 拆分输入、存储和输出 DTO，补 `timeout_secs`。
6. 为每个旧 type 建 fixture，逐字段比较迁移前后原始值。
7. 模拟“升级→旧 schema INSERT/UPDATE→再次升级”，断言 resolver 得到正确身份。
8. 模拟新记录交给旧路由 URL builder，断言兼容调用路径。
9. 单独测试新版两步 UPDATE 的最终 revision 非 0，以及事务中途失败会整体回滚，不留下半新半旧身份。

## 验收标准

- migration 只增列，不改变任何旧业务字段值。
- 旧 payload create/update 不反序列化失败，不清空新字段。
- 旧 Gemini 保持原生执行覆盖，直到用户显式应用新 preset。
- 旧 Ollama 不生成 `/v1/api/chat`。
- 回滚写入产生的 revision 0 行在新版能实时解析和路由。
- 新 Anthropic/Ollama 配置在上一版代码下至少完成对应兼容协议的基本调用。

## 测试命令

- `cargo test channel_identity --manifest-path src-tauri/Cargo.toml`
- `cargo test channel_migration --manifest-path src-tauri/Cargo.toml`

## 交接输出

提供 schema diff、identity resolver 表、trigger 语义、新旧 DTO JSON 示例、每个 preset 的 native/legacy URL 测试表。
