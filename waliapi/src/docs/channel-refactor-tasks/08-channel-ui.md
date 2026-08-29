# T08：渠道表单与列表双标签

## 目标

按原型重构新建/编辑渠道表单，并在渠道浏览列表显示协议、提供商双标签；保留模型映射、优先级、权重、超时和现有操作能力。

## 依赖

- T01 `get_channel_presets()`。
- T02 规范化 Channel/Create/Update DTO。
- T07 `test_channel_draft()`。
- 后端接口未合并时可用类型化 mock 开发，但最终不得保留模板 fallback。

## 文件所有权

- 修改 `src/components/ChannelForm.tsx`，可拆分 `src/components/channel-form/`。
- 修改 `src/pages/ChannelsPage.tsx`。
- 修改 `src/types/index.ts`、`src/lib/api.ts` 的前端消费部分；与 T01/T07 已定义接口保持兼容。
- 修改必要的 `src/App.css` 或局部样式。
- 不修改 Rust、migration、routing。

## 表单交互

1. 顶部 OpenAI、Anthropic、Ollama 三个等宽 Tab，键盘左右切换并有 ARIA tab 语义。
2. 每个协议默认选择顶部整行“自定义配置 / 手动配置协议与 Base URL”。
3. 自定义后依次显示国际、国内、本地厂商卡片；仅显示该协议支持的模板。
4. DeepSeek 标国内；Anthropic 不显示 Moonshot；豆包显示“字节豆包（Coding Plan）”。
5. OpenAI 协议显示 Chat/Responses 两个 checkbox，至少选一个，可双选。
6. Anthropic 显示固定 Messages 及模板声明的 count_tokens 能力；Ollama 显示 `/api/chat`。
7. 自定义不预填 URL、模型、密钥；厂商预设仅在用户选择时应用模板。
8. 协议/厂商切换若 URL、模型或端点已编辑，弹确认；Key、备注、映射、priority、weight、timeout 不被静默清空。
9. 模型列表、模型映射、全局 mapping suggestions、数组 mapping UI 保持现有功能。
10. 编辑旧渠道显示 resolver 的规范化身份和“来自旧配置”；未保存不写回。

## 保存前验证

点击保存先做本地校验，再调用 draft test。展示每端点进度和结果。失败时主按钮“修改配置”，次级危险按钮“仍然保存”；强制保存保留原端点选择。

测试可能产生极少费用的说明必须在操作区域可见。保存中防重复提交，关闭/取消不保存。连接相关字段变更后清除旧 `test_run_id/draft_fingerprint`；真正创建或更新时原样提交当前测试 receipt，不能用前一次草稿结果。

## 渠道列表

名称后严格显示两个标签：第一 `[协议]`，第二 `[提供商]`。例：`[Anthropic] [DeepSeek]`、`[OpenAI] [自定义]`。数据来自规范化 protocol/provider，不直接用旧 type。

Base URL 显示 UI 规范 `native_base_url`；状态、序号、模型数、成功率、延迟、P/W、测试、编辑、启停、删除、展开保持现有功能和位置层级。

未知 legacy identity 显示 `[旧配置] [自定义]` 或后端返回的明确 fallback，不误标为 OpenAI 官方。

## 实施步骤

1. 将表单状态改为 protocol/provider/native endpoints，保留旧业务字段。
2. 接入 presets query，处理 loading/error；加载失败禁止套用模板，但允许显示明确错误，不使用硬编码副本。
3. 实现协议 Tab、自定义卡片和地域厂商网格。
4. 实现预设应用与 dirty-field 确认。
5. 实现协议配置区和端点校验。
6. 接入 draft test modal/result，处理强制保存。
7. 实现测试 receipt 的失效规则：protocol/provider/URL/Key/models/endpoints/timeout 变更即失效；name/note/priority/weight 变更不失效。
8. 复核编辑 Key 的留空/清空语义。
9. 更新渠道卡片双标签和规范 URL。
10. 完成窄屏、键盘、中文输入法、错误状态测试。

## 验收标准

- 三个 Tab 的默认 provider 都是 custom。
- 每个协议的厂商集合准确；切换不静默丢用户字段。
- OpenAI 0 个端点无法提交，1/2 个均可测试。
- 双选产生两个测试结果；失败可修改或强制保存。
- 列表始终显示协议、提供商两个标签。
- 旧渠道编辑不改变 timeout、status、mapping 或 Key。
- 前端不存在渠道 URL/模型模板副本。

## 测试命令

- `pnpm build`
- 项目引入前端测试框架后运行 ChannelForm/ChannelsPage 组件测试；若本次引入，测试 custom 默认、双端点、dirty confirm、强制保存和双标签。

## 交接输出

提供三协议截图、custom/厂商选择截图、测试失败 modal、双标签列表截图、键盘操作说明和前端 build 结果。
