# 1 Interactions API 概览

> 来源：[Google AI for Developers](https://ai.google.dev/gemini-api/docs/interactions-overview?hl=zh-cn)  
> 抓取说明：官方文档 **Interactions API** 版本（页面默认版本）。  
> 原文标题：Interactions API

---

Interactions API 是我们使用 Gemini 模型和智能体进行构建的最简单、最有效的方式。截至 2026 年 6 月，该 API
已正式发布，建议所有新项目使用。虽然现在被视为旧版，但原始 [`generateContent`](https://ai.google.dev/gemini-api/docs/generate-content/text-generation?hl=zh-cn) API 仍然完全受支持。

## 1.1 为什么要使用 Interactions API？

- **适用于所有应用的通用接口**：该 API 设计为适用于所有用例的标准接口，包括单轮文本生成、多模态理解、结构化输出、工具编排和智能体工作流。
- **适用于模型和智能体的单个 API**：一个统一的端点和模式，用于直接调用标准 Gemini 模型以及专用智能体（例如 Deep Research 智能体和自定义托管智能体）。
- **开箱即用的新功能**：例如，使用 `previous_interaction_id` 的可选服务器端对话状态、用于调试和界面呈现的可观测执行步骤，以及使用 `background=true` 的长时间运行任务的[后台执行](https://ai.google.dev/gemini-api/docs/background-execution?hl=zh-cn)。
- **缓存命中率更高，费用更低**：使用多轮对话时，可选的服务器端状态管理可在多轮对话中实现更高效的上下文缓存，从而降低令牌费用。
- **新功能的发布平台**：未来，所有新模型、多模态功能、工具和智能体功能都将在 Interactions API 上发布。

默认情况下，Interactions API 会存储请求，以便您可以使用 `previous_interaction_id`
利用服务器端状态管理功能。您可以通过设置 `store=false` 来选择无状态行为。如需了解详情，请参阅[数据保留](#data-storage-retention)部分。

## 1.2 开始使用

- **设置编码智能体**：连接到 **Gemini 文档 MCP** 并安装
 `gemini-interactions-api` 技能，让您的助理可以直接访问
最新的开发者文档和最佳实践。
[设置编码智能体 →](https://ai.google.dev/gemini-api/docs/coding-agents?hl=zh-cn)
- **从 `generateContent`** 迁移**：如果您有现有集成，
请按照 [迁移指南](https://ai.google.dev/gemini-api/docs/migrate-to-interactions?hl=zh-cn) 过渡到 Interactions API。
- **开始使用**：请参阅 [Interactions API 使用入门
指南](https://ai.google.dev/gemini-api/docs/get-started?hl=zh-cn)。

### 1.2.1 功能指南

通过这些指南了解 Interactions API 的具体功能。您可以使用这些页面上的切换开关在 generateContent 和
Interactions API 之间切换：

- [文本生成](https://ai.google.dev/gemini-api/docs/text-generation?hl=zh-cn)
- [图片生成](https://ai.google.dev/gemini-api/docs/image-generation?hl=zh-cn)
- [图片推理](https://ai.google.dev/gemini-api/docs/image-understanding?hl=zh-cn)
- [音频理解](https://ai.google.dev/gemini-api/docs/audio?hl=zh-cn)
- [视频理解](https://ai.google.dev/gemini-api/docs/video-understanding?hl=zh-cn)
- [文件处理](https://ai.google.dev/gemini-api/docs/document-processing?hl=zh-cn)
- [函数调用](https://ai.google.dev/gemini-api/docs/function-calling?hl=zh-cn)
- [结构化输出](https://ai.google.dev/gemini-api/docs/structured-output?hl=zh-cn)
- [Deep Research 智能体](https://ai.google.dev/gemini-api/docs/deep-research?hl=zh-cn)
- [灵活推理](https://ai.google.dev/gemini-api/docs/flex-inference?hl=zh-cn)
- [优先推理](https://ai.google.dev/gemini-api/docs/priority-inference?hl=zh-cn)

## 1.3 Interactions API 的工作原理

Interactions API 以核心资源 [**`Interaction`**](https://ai.google.dev/api/interactions-api?hl=zh-cn#Resource:Interaction) 为中心。`Interaction`
表示对话或任务中的完整一轮。它充当会话记录，包含互动的整个历史记录，即按时间顺序排列的**执行步骤**
。这些步骤包括模型思考、服务器端或客户端工具调用和结果（例如 `function_call` 和
`function_result`）以及最终的 `model_output`。存储的资源（通过 `interactions.get`
检索）还包括 `user_input` 步骤以提供完整上下文，但 `interactions.create` 响应仅返回模型生成的步骤。

当您调用
[`interactions.create`](https://ai.google.dev/api/interactions-api?hl=zh-cn#CreateInteraction)时，您将
创建一个新的 `Interaction` 资源。

### 1.3.1 服务器端状态管理

您可以在后续调用中使用
`previous_interaction_id` 参数，使用已完成互动的 `id` 继续对话。服务器使用此
ID 检索对话历史记录，从而避免您重新发送整个聊天记录。

`previous_interaction_id` 参数仅使用 `previous_interaction_id` 保留对话历史记录（输入和输出）。其他参数是**互动范围**
的，仅适用于您当前生成的特定互动：

- `tools`
- `system_instruction`
- `generation_config`（包括 `thinking_level`、`temperature` 等）

这意味着，如果您希望这些参数生效，则必须在每个新互动中重新指定这些参数。此服务器端状态管理是可选的；您也可以通过在每个请求中发送完整的对话历史记录，以无状态模式运行。

### 1.3.2 数据存储和保留

默认情况下，API 会存储所有 Interaction 对象 (`store=true`)，以便
简化服务器端状态管理功能（使用
`previous_interaction_id`）、[后台执行](https://ai.google.dev/gemini-api/docs/background-execution?hl=zh-cn)（使用 `background=true`）和
可观测性功能的使用。

- **付费层级**：系统会将互动保留 **55 天**。
- **免费层级**：系统会将互动保留 **1 天**。

如果您不希望这样做，可以在请求中设置 `store=false`。此控制与状态管理分开；您可以选择不存储任何互动。不过，请注意，
`store=false`与[后台执行](https://ai.google.dev/gemini-api/docs/background-execution?hl=zh-cn)不兼容，并且会阻止在后续轮次中使用
`previous_interaction_id`。

对于付费层级项目，您可以在
[AI Studio](https://aistudio.google.com/logs?hl=zh-cn)中配置保留期限，以在 7 天、14 天、28 天或 55 天后自动将日志标记为从项目存储空间中
删除。较短的保留期限可能会影响对过往对话的检索。

您可以使用 [`delete`](https://ai.google.dev/api/interactions-api?hl=zh-cn#deleteInteraction) 方法以编程方式随时删除存储的互动，这
需要互动 ID。您还可以在
[AI Studio](https://aistudio.google.com/logs?hl=zh-cn)中查看和管理存储的互动
日志，包括从项目存储空间中删除。

保留期限到期后，系统会自动删除您的数据。

系统会根据[条款](https://ai.google.dev/gemini-api/terms?hl=zh-cn)处理 Interactions 对象。

### 1.3.3 在 AI Studio 中查看互动

对于付费层级项目，API 会存储使用 `store=true` 执行的 Interactions API 请求。[您可以直接从 Google AI Studio 的“日志”页面查看这些请求。](https://ai.google.dev/gemini-api/docs/www.aistudio.google.com/logs?hl=zh-cn)如需了解详情，请参阅
[日志指南](https://ai.google.dev/gemini-api/docs/logs-datasets?hl=zh-cn)。

## 1.4 最佳实践

- **缓存命中率**：有状态模式和
无状态模式均支持隐式缓存（请参阅
[快速入门](https://ai.google.dev/gemini-api/docs/get-started?hl=zh-cn#4_multi-turn_conversations)）。使用 `previous_interaction_id`（有状态）继续对话可让系统更轻松地利用对话历史记录的隐式缓存，从而提高性能并降低费用。
- **混合互动**：您可以灵活地在对话中混合搭配智能体互动和
模型互动。例如，您可以使用专用智能体（例如 Deep Research 智能体）进行初始数据收集，然后使用标准 Gemini 模型执行后续任务（例如总结或重新格式化），并使用 `previous_interaction_id` 将这些步骤关联起来。

## 1.5 支持的模型和智能体

| 模型名称 | 类型 | 模型 ID |
| --- | --- | --- |
| Gemini 3.5 Flash | 模型 | `gemini-3.5-flash` |
| Gemini 3 Pro 预览版 | 模型 | `gemini-3.1-pro-preview` |
| Gemini 3.1 Flash-Lite | 模型 | `gemini-3.1-flash-lite` |
| Gemini 3 Flash 预览版 | 模型 | `gemini-3-flash-preview` |
| Gemini 2.5 Pro | 模型 | `gemini-2.5-pro` |
| Gemini 2.5 Flash | 模型 | `gemini-2.5-flash` |
| Gemini 2.5 Flash-lite | 模型 | `gemini-2.5-flash-lite` |
| Gemini 3 Pro Image | 模型 | `gemini-3-pro-image` |
| Gemini 3.1 Flash Image | 模型 | `gemini-3.1-flash-image` |
| Gemini 3.1 Flash TTS 预览版 | 模型 | `gemini-3.1-flash-tts-preview` |
| Gemma 4 31B IT | 模型 | `gemma-4-31b-it` |
| Gemma 4 26B MoE IT | 模型 | `gemma-4-26b-a4b-it` |
| Lyria 3 Clip 预览版 | 模型 | `lyria-3-clip-preview` |
| Lyria 3 Pro 预览版 | 模型 | `lyria-3-pro-preview` |
| Deep Research 预览版 | 智能体 | `deep-research-preview-04-2026` |
| Deep Research 预览版 | 智能体 | `deep-research-max-preview-04-2026` |
| Antigravity 预览版 | 智能体 | `antigravity-preview-05-2026` |

## 1.6 SDK

您可以使用最新版本的 Google GenAI SDK 来访问 Interactions API。

- 在 Python 上，这是 `2.3.0` 及更高版本的 `google-genai` 软件包。
- 在 JavaScript 上，这是 `2.3.0` 及更高版本的 `@google/genai` 软件包。

如需详细了解如何在
[库](https://ai.google.dev/gemini-api/docs/libraries?hl=zh-cn)页面上安装 SDK，请参阅此页面。

## 1.7 限制

- **远程 MCP**：Gemini 3 不支持远程 MCP，此功能即将推出。
- **多轮模型兼容性**：在对话中混合使用不同模型（有状态或无状态）时，后续模型必须支持先前模型的输出模态作为输入。例如，如果您使用 `gemini-3.1-flash-image` 生成图片，则无法使用不接受图片输入（例如仅文本模型或 Lyria 等音乐生成模型）的模型继续该对话。

[`generateContent`](https://ai.google.dev/gemini-api/docs/generate-content/text-generation?hl=zh-cn) API 支持以下功能，但 Interactions API 中**尚不提供** 这些功能：

- **[视频元数据](https://ai.google.dev/gemini-api/docs/video-understanding?hl=zh-cn)**：`video_metadata` 字段，用于为视频理解设置剪辑
间隔和自定义帧速率。
- **[批量 API](https://ai.google.dev/gemini-api/docs/batch-api?hl=zh-cn)**
- **[自动函数调用 (Python)](https://ai.google.dev/gemini-api/docs/function-calling?example=meeting&hl=zh-cn#automatic_function_calling_python_only)**
- **[显式缓存](https://ai.google.dev/gemini-api/docs/caching?hl=zh-cn)**：请注意，Interactions API
中可通过 `previous_interaction_id` 使用服务器端隐式缓存。
- **[安全设置](https://ai.google.dev/gemini-api/docs/safety-settings?hl=zh-cn)**：Interactions API 不支持自定义安全
设置。

## 1.8 反馈

您的反馈对于 Interactions API 的开发至关重要。
欢迎在
[Google AI 开发者社区论坛](https://discuss.ai.google.dev/c/gemini-api/4?hl=zh-cn)上分享您的想法、报告 bug 或请求功能。

## 1.9 后续步骤

- 试用 [Interactions API 快速入门笔记本](https://colab.sandbox.google.com/github/google-gemini/cookbook/blob/main/quickstarts/Get_started_interactions_api.ipynb?hl=zh-cn)。
- 详细了解 [Gemini Deep Research 智能体](https://ai.google.dev/gemini-api/docs/deep-research?hl=zh-cn)。
