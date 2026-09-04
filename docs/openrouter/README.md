# OpenRouter Responses API 接入说明

> 状态：已接入 Astro Agent Responses 链路
>
> 更新日期：2026-09-04

## 1. 官方文档

- [Responses API Overview](https://openrouter.ai/docs/api_reference/responses/overview)
- [Basic Usage](https://openrouter.ai/docs/api_reference/responses/basic-usage)
- [Reasoning](https://openrouter.ai/docs/api_reference/responses/reasoning)
- [Tool Calling](https://openrouter.ai/docs/api_reference/responses/tool-calling)
- [OpenAI tools guide](https://developers.openai.com/api/docs/guides/tools)

## 2. 接入方式

OpenRouter 直接复用 Astro 已有的 `OpenAIResponsesModel`，不增加专用 adapter。

现有基础设施已经覆盖：

- `POST {base_url}/responses` 请求；
- 原生 `ResponseItem` 历史；
- instructions、tools、tool choice、parallel tools 与 reasoning；
- function call/output 的 `call_id` 配对；
- usage、错误和终态处理；
- Chat Completions 与 Embedding 独立能力槽位。

本次只补充 OpenRouter 文档中出现的 SSE 事件别名，并打开 Provider capability：

```text
response.content_part.delta  -> Text
response.reasoning.delta     -> Thinking
response.done                -> Usage + Done
```

标准 OpenAI 事件仍然保留：

```text
response.output_text.delta
response.reasoning_summary_text.delta
response.completed
```

这样 OpenAI 与 OpenRouter 共用同一个 Responses 请求和解析管线，不把 Provider 差异泄漏到
Agent runtime。

## 3. 无状态约束

OpenRouter Responses API 是无状态接口：每轮必须发送完整历史，且不支持 `store: true` 或
非空 `previous_response_id`。

Astro 当前 Agent 路径天然符合该约束：

- `ResponsesRequest` 不包含 `previous_response_id`；
- Agent 每轮发送 canonical `Vec<ResponseItem>` 历史；
- 默认请求不发送 `store: true`；
- Provider 失败不会自动降级到 Chat Completions。

因此当前接入不需要新增 OpenRouter 请求 adapter。若未来允许用户从
`additional_params` 注入状态字段，应在通用参数校验层统一拒绝，而不是为 OpenRouter
复制一套请求实现。

## 4. Provider 能力

OpenRouter profile 现在使用：

- `api_mode: ApiMode::Responses`；
- `supports_responses: true`；
- 默认地址 `https://openrouter.ai/api/v1`；
- 默认模型 `openai/gpt-5.6`。

`dispatch.rs` 已有的 `attach_responses::<OpenRouter>` 分支会自动挂载 Responses model，
Desktop、辅助模型、Dreaming 和 Evolution 选择器则继续沿
`supports_responses_api` 使用同一个 capability 事实源。

OpenRouter 的具体模型是否支持 reasoning、tools、文件或多模态，仍以模型元数据和实际
上游能力为准；Provider 支持 Responses 不代表每个模型支持全部特性。

## 5. 验证范围

自动化测试覆盖：

- OpenRouter 文本 delta；
- OpenRouter reasoning delta；
- `response.done` 的 usage 与完成状态；
- OpenRouter profile 默认使用 Responses；
- registry 同时保留 Embedding 与 Responses；
- Tauri 暴露 OpenRouter 的 Agent Responses capability；
- 原有 OpenAI Responses 事件保持兼容。

没有 API Key 时不执行 live smoke test。需要线上验证时，建议选择同时支持 tools 与
reasoning 的低成本模型，并确保 API Key 不进入 fixture、日志或仓库。

## 6. 已知边界

OpenRouter 文档允许 `function_call_output.output` 使用 `input_file`。Astro 当前 canonical
tool output 已支持字符串、`input_text`、`input_image` 和 `input_audio`，尚未支持
`input_file`。这不阻塞文本、reasoning 和常规工具调用，但不能宣称文件型 tool output
已完整覆盖。
