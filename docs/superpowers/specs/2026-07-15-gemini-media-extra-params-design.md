# Gemini 媒体工具扩展参数（出图 / 视频）

日期：2026-07-15  
状态：已实现  
背景：Gemini OpenAI 兼容层提供多项专有字段；本轮只接高价值、可用工具参数表达的项。

## 范围

| 工具 | 新参数 | 行为 |
|------|--------|------|
| `image_gen` | `aspect_ratio`（可选） | Google 兼容 `images/generations` 请求体带上；OpenAI 忽略 |
| `video_gen` | `resolution`、`negative_prompt`（可选） | 与现有 `aspect_ratio` / `duration_seconds` 同为 multipart 字段 |

## 非目标

- `extra_body` 裸透传、`cached_content`、`safety_settings`、图生视频 / 参考图流水线
- 聊天 `thinking_config`（已有 thinking UI）

## 约定

- Agent 只见命名参数，不接触 `extra_body` JSON
- 空/缺省 = 不发送该字段
