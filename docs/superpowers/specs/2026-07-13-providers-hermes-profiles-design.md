# Providers Hermes Profiles 重构

**日期:** 2026-07-13  
**状态:** 已实现  
**范围:** `providers` crate + Tauri 默认 endpoint / 模型列表；不改 agent HITL/编排

## 目标

对齐 Hermes：`ProviderProfile` 表驱动 + 三种 `ApiMode`，chat 路径不再按厂商复制协议分叉。

## ApiMode

| Mode | 用途 |
|------|------|
| `ChatCompletions` | OpenAI 兼容 SSE（含 Gemini OpenAI compat、Ollama `/v1`、Azure deployment quirk） |
| `AnthropicMessages` | Anthropic Messages SSE |
| `Responses` | OpenAI Responses（骨架；默认 profile 不绑定） |

## 关键默认 base

| id | default_base_url | auth | notes |
|----|------------------|------|-------|
| google | `https://generativelanguage.googleapis.com/v1beta/openai` | Bearer | 删除 native `generateContent` chat |
| ollama | `http://localhost:11434/v1` | None | 删除 NDJSON `/api/chat` |
| azure | `""`（须用户填） | AzureHeader | deployment URL quirk |
| claude | `https://api.anthropic.com` | AnthropicKey | Messages |
| 其余 OpenAI 兼容厂商 | 原默认 | Bearer | 不变 |

## 删除清单

- `google_chat_stream`（native SSE）
- `ollama_chat_stream` NDJSON 分支
- verify 中 Google `generateContent`、Ollama `/api/chat`（改走 Chat Completions）

## Breaking

- 自定义 Google native endpoint 的用户需改为 OpenAI 兼容 base（`.../v1beta/openai`）
- Ollama 非 `/v1` endpoint 需改为 `http://host:11434/v1`
- `AuthKind::for_provider("google")` 改为 `Bearer`（`GoogleQuery` 仅保留给出图等原生路径）

## 非目标

LiteLLM 代理层、agent 编排、image_gen 协议统一进三 ApiMode。
