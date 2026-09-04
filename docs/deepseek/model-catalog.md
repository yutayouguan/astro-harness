# DeepSeek Codex 模型目录映射

> 官方来源：[DeepSeek 接入 Codex](https://api-docs.deepseek.com/zh-cn/quick_start/agent_integrations/codex)
>
> Astro 同步日期：2026-09-04

DeepSeek 官方要求 Codex 通过 `model_catalog_json` 读取模型级能力。Astro 采用相同分层：
Provider 声明 Responses/namespace 上限，`agent-types::ModelProfile` 声明具体模型能力。

## 内置模型

| 模型 | 上下文 | 推理档位 | Tool Search | 输入模态 |
| --- | ---: | --- | --- | --- |
| `deepseek-v4-flash` | 1,048,576 | low / high / max | 支持 | text |
| `deepseek-v4-pro` | 1,048,576 | low / high / max | 支持 | text |
| `deepseek-v4-flash-vision-exp` | 1,048,576 | low / high / max | 支持 | text / image |

共同运行时能力：

- 默认 reasoning effort 为 `high`；
- `supports_search_tool: true`；
- 支持并行工具调用；
- `apply_patch` 使用 freeform；
- Web Search 为 text；
- verbosity 默认 `low`；
- 有效上下文比例为 95%；
- reasoning summary 可用；
- Multi-Agent 使用 V2。

## Astro 映射边界

Astro 不复制官方 JSON 中的 Codex 专属 `model_messages` / `base_instructions`，继续使用自己的
PromptContract。当前 Codex 已不消费的旧字段也不进入 Astro 运行时。官方目录负责能力与
上下文，DeepSeek `/models` 负责可用模型列表，OpenRouter 只作为价格等补充元数据来源。

未知 DeepSeek 模型不会继承上述三项精确模型的 Vision、reasoning 档位或上下文配置。
