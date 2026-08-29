# T14：上游模型同步（同步上游模型 + 弹窗勾选应用）

> **状态:已验收原型**（交互确认,真实前后端尚未实现）

## 目的

启用渠道编辑页被硬编码禁用的「同步上游模型」按钮（`ChannelForm.tsx:694-696`,
commit `30513a2` 引入的 `disabled`）。点击后**按协议拉取上游模型列表**,在弹出的对话框中
**搜索 / 全选 / 已有标记**,勾选后「应用到渠道」**合并去重**进当前编辑的模型列表。

这一交互已通过纯 HTML 原型验收（`prototype/model-sync.html`）。本任务把原型固化为
真实前后端实现计划。

## 交互（已验收,原型为准）

验收确定的 MVP 交互:**弹窗列出,勾选后应用**。

流程:

1. 渠道卡片点「⟳ 同步上游模型」→ 按钮转 loading「正在拉取…」。
2. 后端按协议拉取上游模型 ID 列表,返回。
3. 弹窗列出全部模型:
   - 顶部搜索框（实时过滤）
   - 「全选」复选（勾/取消全部）
   - 每行 checkbox + 模型 ID（等宽字体）+ 标签:**「已有」**（当前模型列表已含）/**「新增」**
   - 计数:`已选 N / 总数,新增 M 个`
4. 已有模型**默认勾选**;底部「应用到渠道」按钮带 `+M`（M=新增数）;M=0 时按钮禁用。
5. 应用 = 合并去重（保序 + 新增追加 → `new Set([...existing, ...selected])`）;
   新增 chip 弹出绿色高亮动画 + toast「已添加 M 个模型到「渠道名」」。
6. 关闭:Escape / 遮罩点击 / 取消。空列表显示「没有匹配的模型」。
7. 空模型列表（0 个）表示通配「接受所有模型」——同步到的结果**合并**进空列表,
   不改其通配语义。

原型文件:[14-model-sync-prototype.html](14-model-sync-prototype.html)（自 `prototype/model-sync.html` 移入）

## 后端方案

### 命令

注册 tauri command `sync_upstream_models`（`src-tauri/src/commands/channel.rs` +
`src-tauri/src/lib.rs:133` 的 `invoke_handler(generate_handler![...])`）。

签名（草拟）:

```rust
#[tauri::command]
async fn sync_upstream_models(
    state: State<'_, AppState>,
    draft: DraftChannel,          // 编辑态草稿(含 base_url / api_key,可能未落库)
) -> Result<SyncUpstreamModelsResult, String>;
```

- 复用渠道测试的草稿语义:**空 api_key 时从已存 channel 回填**（`resolve_draft_api_key`,
  `services/channel_test.rs:397`;`clear_api_key == Some(true)` 返回空）。
- 返回:模型 ID 数组 + 来源协议（供前端打标）。

### 2. 按协议拉取上游模型列表

新增 `services/upstream_models.rs`,按协议分派（复用 `get_adaptor` 的协议判定）:

| 协议 | 接口 | 解析 | 认证 header |
|---|---|---|---|
| OpenAI / Anthropic（`GET {base}/models`） | `GET {base}/models` | `data[].id` | `Authorization: Bearer <key>`(openai/custom);`x-api-key` + `anthropic-version`（claude/anthropic） |
| Ollama | `GET {base}/api/tags` | `models[].name` | 通常无鉴权 |

> 核实:OpenCode-GO 25 个模型、9router-tp 37 个模型均为 OpenAI 格式 `data[].id`
> （9router-tp 是 Anthropic 协议但返回 OpenAI 格式,无 capabilities）。Ollama 用 `models[].name`。

错误处理:拉取失败返回可读错误,不覆盖已有模型列表;「失败时不会覆盖已有模型列表」是
原按钮 `title` 的承诺,保持。

### 3. 与 `ModelEnumStrategy` 预留对接

`channel_presets.rs` 已有枚举:

```rust
enum ModelEnumStrategy { static_only, static_plus_sync, sync_only }
```

当前渠道列表默认 `static_only`。此功能启用的是 **`static_plus_sync`** 方向:静态列表 +
可同步合并。未来 `sync_only`（纯同步、不支持手填静态列表）可作为后续窗口,本任务不做。

### 4. 失败语义

- 拉取失败:toast/错误提示,模型列表不动。
- 幂等:合并是去重 union,重复点击安全。不做覆盖式替换（不强制整表重填）。

---

## 前端接线（真实实现,接在原型之后）

`src/components/ChannelForm.tsx:694-696` 去掉 `disabled`,接线:

1. `src/lib/api.ts` 新增 `channelApi.syncUpstreamModels(draft)` → 调 `sync_upstream_models` 命令。
2. 新增 `ModelSyncModal` 组件（参考原型 `prototype/model-sync.html` + `DraftTestModal.tsx` 的
   overlay/surface 模式;token:`background #f5f7fa` / `primary #2f6fed` / `success #1f8f5f` /
   `muted-foreground #66758a`;ts 序列化字符串须与 Rust 枚举 serde 输出一致）。
3. `ChannelForm` 点按钮 → 调命令 → 打开 modal（loaded 才打开,loading 态在按钮上）。
4. 弹窗勾选 → 关闭时把新增合并进 `form.models`（set 去重保序）;渲染新增 chip 高亮 + toast。

### 前端状态机

`idle → loading → loaded(GET) → 弹窗选择 → applied / cancelled / error`

---

## 关键文件清单

| 文件 | 改动 |
|---|---|
| `src-tauri/src/commands/channel.rs` | 新增 `sync_upstream_models` 命令 |
| `src-tauri/src/lib.rs` | `invoke_handler` 注册该命令 |
| `src-tauri/src/services/upstream_models.rs` | 新增:协议分派拉取 + 解析 |
| `src-tauri/src/services/channel_test.rs` | 复用 `resolve_draft_api_key` |
| `src-tauri/src/channel_presets.rs` | `ModelEnumStrategy` 已预留,接线 |
| `src/components/ChannelForm.tsx` | 解除 `disabled`,接线同步流程 |
| `src/lib/api.ts` | `channelApi.syncUpstreamModels` |
| `src/components/.../ModelSyncModal` | 新增弹窗组件 |
| `src-tauri/src/rollout_integration_tests.rs` | `MockUpstream`(行 104/118/213)模拟 `/models` / `/api/tags` 断言 |

## 验证

1. `cargo test -p wali-api`（新 upstream_models 单测 + 集成）。
2. 集成:用沿用 `MockUpstream` 起点驱动协议分派,断言:
   - OpenAI/Anthropic 解析 `data[].id`、Ollama 解析 `models[].name`;
   - 认证 header 按协议正确;
   - 空 api_key 从已存 channel 回填;
   - 拉取失败返回可读错误、不覆盖原列表。
3. 手动:各协议渠道点「同步」→ 弹窗列模型 → 勾选新增 → 应用后 chip 高亮 + 合并、无重复;
   Escape / 遮罩 / 取消关闭;搜索 / 全选 / 已有标记正确;空结果提示。

## 未决风险 / 后续窗口

- Anthropic 官方 `/models` 带 `capabilities`,本任务只取 `id`,不按能力过滤（上游已有语义; API 返回策略）。
- `sync_only`（纯同步、无本地静态列表的编辑态）留后续,含 undo 编辑窗口。
- 下拉覆盖过多 `GET /models` 大列表时的 UX:弹窗内搜索 + 全选已覆盖 MVP;超过场景（如
  >500 条）可见滚动条 + 计数打点,不做分页（后续按需）。