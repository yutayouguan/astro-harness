# 1 Gemini Interactions API 教程（本地镜像）

本目录收录 [Google Gemini Interactions API](https://ai.google.dev/gemini-api/docs/interactions-overview?hl=zh-cn) 官方文档的中文镜像，便于在仓库内查阅与迁移参考。

> **说明**
>
> - 来源均为 Google AI for Developers 官方文档的 **Interactions API** 版本（页面默认版本）。
> - 抓取时间：2026-07-16。
> - 官方文档会持续更新；若与线上不一致，以官方页面为准。
> - SDK 要求：Python `google-genai >= 2.3.0`，JavaScript `@google/genai >= 2.3.0`。

## 1.1 阅读顺序（推荐）

| 顺序 | 文档 | 说明 |
| --- | --- | --- |
| 0 | [Interactions API 概览](./00-overview-interactions-api.md) | 为什么用、工作原理、状态管理、模型列表、限制 |
| 1 | [迁移指南](./01-migrate-to-interactions.md) | 从 `generateContent` 迁到 Interactions API |
| 2 | [使用入门](./02-get-started.md) | API Key、首个调用、流式、多轮、工具与智能体 |

## 1.2 功能指南

| 文档 | 官方原文 |
| --- | --- |
| [文本生成](./03-text-generation.md) | [text-generation](https://ai.google.dev/gemini-api/docs/text-generation?hl=zh-cn) |
| [图片生成](./04-image-generation.md) | [image-generation](https://ai.google.dev/gemini-api/docs/image-generation?hl=zh-cn) |
| [图片理解 / 图片推理](./05-image-understanding.md) | [image-understanding](https://ai.google.dev/gemini-api/docs/image-understanding?hl=zh-cn) |
| [音频理解](./06-audio-understanding.md) | [audio](https://ai.google.dev/gemini-api/docs/audio?hl=zh-cn) |
| [视频理解](./07-video-understanding.md) | [video-understanding](https://ai.google.dev/gemini-api/docs/video-understanding?hl=zh-cn) |
| [文件 / 文档处理](./08-document-processing.md) | [document-processing](https://ai.google.dev/gemini-api/docs/document-processing?hl=zh-cn) |
| [函数调用](./09-function-calling.md) | [function-calling](https://ai.google.dev/gemini-api/docs/function-calling?hl=zh-cn) |
| [结构化输出](./10-structured-output.md) | [structured-output](https://ai.google.dev/gemini-api/docs/structured-output?hl=zh-cn) |
| [Deep Research 智能体](./11-deep-research.md) | [deep-research](https://ai.google.dev/gemini-api/docs/deep-research?hl=zh-cn) |
| [灵活推理 (Flex)](./12-flex-inference.md) | [flex-inference](https://ai.google.dev/gemini-api/docs/flex-inference?hl=zh-cn) |
| [优先推理 (Priority)](./13-priority-inference.md) | [priority-inference](https://ai.google.dev/gemini-api/docs/priority-inference?hl=zh-cn) |
| [Astro 实现备注](./14-astro-implementation-notes.md) | 本仓库线上 wire / 修复约定（非官方镜像） |

## 1.3 与本仓库的关系

Astro 中 Google / Gemini 相关调用计划统一迁移到 Interactions API（`interactions.create`），以对齐：

- 统一模型与智能体入口
- `previous_interaction_id` 服务端会话状态
- 可观测 `steps`（thought / function_call / model_output）
- `background=true` 长任务
- 更高缓存命中率与更低多轮成本

迁移时优先对照 [01-migrate-to-interactions.md](./01-migrate-to-interactions.md) 与各功能章节中的 Python / JavaScript / REST 示例。

**Astro 落地与线上 wire 差异**（工具入参 `{}`、`arguments_delta`、`function_call.signature`、`thought_signature`、usage 字段等）见：

- [14 · Astro 实现备注](./14-astro-implementation-notes.md)

## 1.4 关键概念速查

```text
interactions.create(...)
  ├─ model / agent
  ├─ input（文本或多模态 content）
  ├─ previous_interaction_id?   # 有状态多轮
  ├─ store?                     # 默认 true；false 则无状态
  ├─ system_instruction?        # 每轮需重传（interaction-scoped）
  ├─ tools?                     # 同上
  ├─ generation_config?         # 同上（thinking_level / temperature …）
  ├─ stream? / background?
  └─ service_tier?              # flex / priority（可选）

响应 Interaction
  ├─ id / status / usage
  ├─ steps[]                    # thought / function_call / model_output …
  └─ SDK 便捷字段：output_text / output_image / …
```
