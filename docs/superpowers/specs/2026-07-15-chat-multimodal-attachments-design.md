# 聊天附件多模态（OpenAI 兼容 image_url）

日期：2026-07-15  
状态：已实现  
参考：[Gemini OpenAI — 图片理解](https://ai.google.dev/gemini-api/docs/openai?hl=zh-cn#javascript_4)；[`extra_body`](https://ai.google.dev/gemini-api/docs/openai?hl=zh-cn#extra-body)（聊天侧仅 cached_content / thinking_config，**不用来塞附件**）

## 目标

用户消息中的**图片附件**以 `messages[].content` 数组形式发给多模态模型（`text` + `image_url` data URL），替代把 base64 拼进纯文本。

## 非目标

- `extra_body` 传图；`cached_content` 产品化  
- PDF/视频解码进 chat；历史消息 DB 结构化 parts 迁移  

## 设计

1. `providers::ChatMessage`：可选 `parts: Vec<ContentPart>`（`Text` / `ImageUrl { url }`）；`content` 仍为文本（UI/摘要）
2. `to_openai_messages`：有 parts → content 数组；否则 string
3. Tauri：从附件构建 parts；非图附件仍拼文本；超大图仅元数据
4. Google + thinking：可选经 `additional_params` / `extra_body.google.thinking_config`（不阻塞看图）

## 验收

- [x] 带图聊天走 content 数组（可观测请求或手动验）
- [x] 无图行为不变
- [x] 超大图不塞崩溃级 body
