# 架构师 · 方案设计（architect-design）

> 模型 `gpt-5.6-sol`（high）｜需求已确认，你产出设计文档 + 任务拆分。**范围基线 = 优化需求书 §2.1**。

## 输入
- **`docs/auth-codex/work/00-optimized-requirements.md`**（范围基线；§3.2/3.3/3.4 已给出最小改动面）
- 已拍板的 `docs/auth-codex/work/01-requirements-review.md`（若有新增决策）
- 需求文档全集（`docs/auth-codex/`，注意 `route_plan.rs`/`attempt.rs` 在 `core/` 下）
- 代码现状（`src-tauri/src/`、`src/`）

## 任务
1. 设计覆盖（**每个 ADR + 优化需求书 §2.1 条目必须有落点**）：
   - **数据层**：`auth_accounts` 表迁移（通用列 + payload_json，ADR-3/13）、`request_logs.upstream_type`（ADR-30）、QuotaState / model_states 语义（ADR-16）
   - **provider 抽象**：`Provider` trait（登录/刷新/出站，ADR-12），codex 实现（OAuth PKCE + localhost 回调 + auth.json 导入 + backend-api adapter）
   - **路由层**：混合候选池（§3.2 候选泛化的全部消费点——不止 4 处，另含 `authorize_and_plan`/handler/`plan_executor::AttemptMeta`/`debug_json`）、账号过滤（QuotaState/失效/停用）、`classify_channel` 账号分支对 **Chat/Messages/Responses 均出组**（D-A）
   - **出站适配**：账号出站分支（§3.4）、懒刷新 + 401 重试（适配器内部，D-3）、限额响应头解析（ADR-15）、**字段 allowlist + 强制 stream:true 兜底 + rate_limits 原样透传（保守约束）**；zstd 不实现（D-E）
   - **Codec（ADR-31 / D-A）**：Responses→Chat 流式状态机 + 非流 decoder（§3.3 复用面/从零写清单）、registry + `SseMode` 接线、严格 fail-closed 请求编码、Native 直通 usage 补提取（D-4）
   - **定时任务**：单个 12h 后台循环（令牌刷新 + 模型同步 + 失效重试，D-F）；30min 探测不做（D-C）
   - **Tauri 命令集**：ADR-20 的 10 条命令（DTO 不返回 access_token / refresh_token / id_token / payload_json 全文）
   - **前端**：`/channels` + `/channels/auth` 双路由（ADR-4，注意 Sidebar 前缀匹配）、Auth 页（01-ui-spec 各节，颜色 token 按实际 `src/App.css` 走）、卡片/弹窗/状态变体、风险 banner（ADR-29）
   - **错误处理与测试策略**：每层如何测、mock 边界（**不依赖真实令牌**）；前端构建以 `npm run build`（仓库根目录）为验收
2. 任务拆分：任务卡模板见下；标依赖拓扑与可并行项。
3. 对 open 风险（优化需求书 §2.3 待验证项）给 v1 保守处置。

## 产出 schema

### 02-design.md
```markdown
# 设计文档
## 1. 架构总览（数据流 + 模块边界）
## 2. 数据层（auth_accounts 表、upstream_type 迁移、DTO 掩码）
## 3. Provider 抽象与 codex 实现
## 4. 路由层改动（候选泛化、classify_channel 账号分支、过滤）
## 5. 出站适配（分叉点、懒刷新/401、限额解析、保守约束）
## 6. Codec（Responses→Chat、registry/SseMode 接线、usage 提取）
## 7. 定时任务（12h 单循环）
## 8. Tauri 命令集
## 9. 前端（路由、Auth 页、卡片、弹窗、状态变体）
## 10. 错误处理与测试策略
## 11. open 风险处置
```

### 03-task-breakdown.md（每张任务卡）
```markdown
## T#：<标题>
- 目标：
- 涉及文件：
- 改动点：
- 验收标准：（可执行命令/断言，无模糊词）
- 依赖：T#
- 独立 agent：是/否
```

## 自检（提交前）
- [ ] 优化需求书 §2.1 每条有落点
- [ ] 每个 ADR 有落点（对照 ADRs.md 决策索引）
- [ ] 任务卡验收标准不含模糊词
- [ ] 覆盖优化需求书 §3.2/3.3/3.4 的最小改动面
