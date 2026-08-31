# Responses API — Provider 参考

各厂商 Responses API 的官方文档链接与接入要点。新增或更新 provider 时读此文件。

## 统一接入架构

所有 Responses API provider 走同一路径：

1. `register_provider()` 正常注册（Chat Completions + 全部媒体能力）
2. `upgrade_to_responses()` 替换 completion model 为 `OpenAIResponsesModel<Ext>`
3. `OpenAICompatible::responses_base_url()` 处理 base_url 变换（仅 Azure 需要覆盖）

新增并默认使用 Responses 的 provider 需要：`profile.rs` 设
`api_mode: ApiMode::Responses` 和 `supports_responses: true`，再在 `dispatch.rs` 的 upgrade match 加一行。
`ChatCompletions` 仅作为用户显式选择的兼容模式。

## Provider 参考表

### OpenAI

- **文档**: https://platform.openai.com/docs/api-reference/responses
- **端点**: `POST /v1/responses`
- **认证**: `Authorization: Bearer sk-xxx`
- **特殊**: `store: false`（平台专有）、`parallel_tool_calls`、`reasoning.summary: "auto"`

### DeepSeek

- **文档**: https://api-docs.deepseek.com/
- **端点**: `POST /v1/responses`（兼容 OpenAI Responses）
- **认证**: `Authorization: Bearer sk-xxx`
- **特殊**: `reasoning_content` 字段（thinking 格式 `ThinkingFormat::DeepSeek`）

### Azure OpenAI

- **文档**: https://learn.microsoft.com/en-us/azure/foundry/openai/how-to/responses
- **端点**: `POST /openai/v1/responses`（注意不是旧的 `/deployments/{id}/` 路径）
- **认证**: `api-key` header（非 Bearer）
- **base_url 变换**: `responses_base_url()` 追加 `/openai/v1`
- **model 字段**: deployment name（与 Chat Completions 不同，model 留在 body 中，不在 URL）
- **特殊**: `content_filters` 扩展字段（Azure 专有）、默认 store 30 天

### 百炼 (Qwen / DashScope)

- **文档**: https://platform.qianwenai.com/docs/api-reference/chat/openai-responses
- **端点**: `POST /compatible-mode/v1/responses`
- **认证**: `Authorization: Bearer sk-xxx`（DashScope API Key）
- **特殊**:
  - `enable_thinking` 需通过 `extra_body` 传递，推荐用 `reasoning.effort`（7 级：none/minimal/low/medium/high/xhigh/max）
  - `x-dashscope-session-cache` header 控制服务端上下文缓存（≥1024 tokens，5min TTL）
  - `store` 默认 true（7 天保留）
  - `background` 不支持（仅同步调用）

### MiniMax

- **文档**: https://platform.minimaxi.com/document/Responses
- **端点**: `POST /v1/responses`
- **认证**: `Authorization: Bearer xxx`
- **特殊**: `reasoning_details` 数组格式（`ThinkingFormat::MiniMaxAdaptive`）、全媒体能力（video/music/TTS）

### Mimo

- **端点**: `POST /v1/responses`（OpenAI 兼容）
- **认证**: `Authorization: Bearer xxx`

## 潜在可接入（尚未启用）

| Provider | Responses API | 备注 |
|---|---|---|
| Ollama | 支持（Open Responses 首批） | base_url `http://localhost:11434/v1` |
| OpenRouter | 支持（Open Responses 首批） | 需验证工具调用兼容性 |
| Volcengine | 未知 | 需查证 |
| Moonshot | 未知 | Kimi K3 仅 Chat Completions 兼容 |
| NVIDIA NIM | 未知 | OpenAI 兼容但未确认 Responses |
| Hunyuan | 未知 | 需查证 |

## 更新此文件

接入新 provider 的 Responses API 后，在上方添加对应条目。关键记录：
- 官方文档 URL
- 端点路径
- 认证方式
- 与标准 OpenAI Responses API 的差异
