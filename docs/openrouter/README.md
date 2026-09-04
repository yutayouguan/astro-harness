# OpenRouter Responses API 接入建议

> 状态：设计建议，尚未启用运行时能力
>
> 调研日期：2026-09-04

## 1. 结论

OpenRouter 可以接入 Astro 的 Agent Responses 链路，但不应只修改
`ProviderProfile.supports_responses`。现有代码已经具备大部分基础设施：

- OpenRouter 已注册 Chat Completions 与 Embedding；
- `dispatch.rs` 已预留 OpenRouter 的 `attach_responses` 分支；
- Agent 历史已经使用原生 `ResponseItem`；
- `OpenAIResponsesModel` 已覆盖请求体、工具、reasoning、usage 与 SSE 基础解析。

真正的阻塞点是 OpenRouter 的 Responses 协议并非与 OpenAI 完全同构。按当前官方文档，
它是无状态转换层，并且部分 SSE 事件名与 Astro 当前 OpenAI 解析器不同。建议把它作为
“OpenAI Responses 兼容但有明确方言”的 Provider 接入，而不是直接打开能力开关。

## 2. 官方文档

- [Responses API Overview](https://openrouter.ai/docs/api_reference/responses/overview)
- [Basic Usage](https://openrouter.ai/docs/api_reference/responses/basic-usage)
- [Reasoning](https://openrouter.ai/docs/api_reference/responses/reasoning)
- [Tool Calling](https://openrouter.ai/docs/api_reference/responses/tool-calling)
- [OpenAI tools guide](https://developers.openai.com/api/docs/guides/tools)

## 3. 已确认的协议差异

| 领域 | OpenRouter 文档契约 | Astro 当前实现 | 建议 |
| --- | --- | --- | --- |
| 状态 | 仅无状态；`store: true` 或非空 `previous_response_id` 返回 400 | Agent 已发送完整历史；通用 adapter 只对部分 Provider 强制 `store: false` | OpenRouter 强制 `store: false`，本地拒绝非空 `previous_response_id` |
| 文本流 | 示例使用 `response.content_part.delta` | 当前解析 `response.output_text.delta` | 增加 OpenRouter 专用映射，并保留 OpenAI 事件 |
| reasoning 流 | `response.reasoning.delta` | 当前解析 `response.reasoning_summary_text.delta` | 两种事件都覆盖，fixture 分开验证 |
| 终态 | 示例使用 `response.done` | 当前只把 `response.completed` / `response.incomplete` 视为终态 | 映射 `response.done`，否则 `[DONE]` 会被判为截断 |
| reasoning item | `type=reasoning`，含 `encrypted_content` 与 `summary` | `ResponseItem::Reasoning` 已可承载 | 验证 `output_item.done` 原样落盘并在下一轮回放 |
| 工具调用 | `function_call` 通过 `call_id` 与 `function_call_output` 配对 | canonical `ResponseItem` 已保留 item id 与 call id | 复用现有链路，补 OpenRouter fixture |
| 工具结果 | `output` 可为字符串或 `input_text` / `input_image` / `input_file` | Astro 已支持字符串、`input_text`、`input_image`、`input_audio`，尚无 `input_file` | MVP 验证现有形态；文件结果作为独立协议扩展，不宣称完整支持 |
| 并行工具 | 文档声明支持并行工具 | adapter 已支持 `parallel_tool_calls` | 由请求显式控制，不对所有模型强制开启 |

文档示例不是线上所有模型和上游 Provider 的能力保证。Responses 端点可用性是
Provider 级能力；reasoning、tools、文件和多模态仍应以模型元数据及实测结果为准。

## 4. 推荐实现方式

### 4.1 将 OpenRouter 从“零差异宏实现”提升为显式适配器

`openai_compat!` 当前会自动生成默认 `OpenAIResponsesCompatible` 实现，无法表达
OpenRouter 的无状态约束和 SSE 方言。建议让 `impls/openrouter.rs` 显式实现：

- `ProviderExt`；
- `OpenAICompatible`，继续保留 Chat/Embedding；
- `OpenAIResponsesCompatible`，声明无状态策略及响应事件适配；
- 现有 `Capabilities`。

不建议把 OpenRouter 特例散落在 `dispatch.rs` 或按 provider id 写条件分支。

### 4.2 为 Responses trait 增加响应解析 hook

请求差异已经由 `OpenAIResponsesCompatible` 表达，响应差异也应留在同一契约中。可增加
默认使用 OpenAI 解析器的 hook，OpenRouter 只覆盖事件归一化：

```text
OpenRouter SSE
  response.content_part.delta  -> Text
  response.reasoning.delta     -> Thinking
  response.output_item.*       -> canonical ResponseItem / ToolCall
  response.done                -> Usage + Done
```

不要在 Agent runtime 中识别 OpenRouter 事件；Provider 层应输出统一 `StreamChunk`。

### 4.3 无状态策略必须在 passthrough 参数之后执行

`additional_params` 可能重新注入 `store` 或 `previous_response_id`。建议新增
`STATELESS_ONLY` 一类明确能力，并在合并扩展参数后执行最终校验：

- 固定 `store: false`；
- 非空 `previous_response_id` 在本地返回清晰错误；
- 始终发送完整 canonical history；
- 不把 OpenRouter 400 静默降级为 Chat Completions。

### 4.4 最后再打开 capability

只有请求与 SSE fixture 通过后，才修改：

- `profile.rs`：OpenRouter 默认 `ApiMode::Responses`；
- `profile.rs`：`supports_responses: true`；
- profile/UI 测试中的 Responses-capable Provider 列表；
- `crates/agent-providers/RESPONSES-API.md` 的能力矩阵。

这样 Desktop 的 Provider、辅助模型、Dreaming 和 Evolution 选择器会沿现有
`supports_responses_api` 链路自动对齐，无需另造前端开关。

## 5. 分阶段实施

### Phase 1：契约与解析器

1. 显式实现 OpenRouter Responses adapter；
2. 强制无状态请求；
3. 支持 OpenRouter 文档中的文本、reasoning、tool call、usage 和终态 SSE；
4. 保留现有 OpenAI Responses 行为不变。

### Phase 2：能力启用与跨层对齐

1. 将 OpenRouter profile 切到 Responses；
2. 打开 `supports_responses`；
3. 更新 Tauri DTO/UI capability 测试；
4. 更新 Provider 架构与能力文档。

### Phase 3：验证与回归

最低测试矩阵：

- 单轮文本，流式与非流式结构 fixture；
- reasoning delta、reasoning item 与 encrypted content 回放；
- 单工具与并行工具调用；
- `function_call` → `function_call_output` → 最终回答；
- 字符串与现有结构化 tool output；
- `input_file` 尚未支持的显式回归/能力边界；
- `response.done` usage 与终态；
- `store: true`、`previous_response_id` 的本地拒绝；
- 400/401/429 不回退到 Chat；
- 首个可见 chunk 前才允许切换 Responses fallback；
- OpenAI、Azure、DeepSeek 等现有 Responses fixture 全部保持通过。

建议再用 OpenRouter 低成本、支持工具与 reasoning 的模型做一次可选 live smoke test；
API Key 不进入测试 fixture、日志或仓库。

## 6. 预计改动范围

核心改动集中在：

- `crates/agent-providers/src/impls/openrouter.rs`
- `crates/agent-providers/src/compat/responses.rs`
- `crates/agent-providers/src/openai/responses.rs` 或新增 OpenRouter SSE parser
- `crates/agent-providers/src/profile.rs`
- `crates/agent-providers/src/dispatch.rs` 的相关测试
- `crates/agent-providers/RESPONSES-API.md`
- Desktop Provider capability 测试

文本、reasoning 与常规工具调用接入不需要修改 Agent 历史模型、Session/rollout schema、
工具执行器或前端主流程。若本轮要求补齐 OpenRouter 的 `input_file` tool output，则需要先在
`agent-protocol` 增加 canonical content variant，再贯通持久化与回放；不应把它伪装成文本。
除此之外若仍需修改这些层，应先停下来重新确认协议边界，避免把 Provider 方言泄漏到
runtime。

## 7. 推荐决策

建议采用“专用轻量 adapter + 共享 canonical Responses pipeline”的方案，并分两次可独立
回滚的提交完成：第一笔只增加适配与 fixture，第二笔打开 capability 并更新跨层文档。
不建议把 OpenRouter 当作完全等价的 OpenAI base URL，也不建议在失败时自动回退到
Chat Completions。
