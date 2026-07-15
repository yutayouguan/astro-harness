# TTS Interactions API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将 Google `tts` 升级到 Gemini Interactions API（单/多说话人、style、stream 落盘），OpenAI `/audio/speech` 保持独立备用。

**Architecture:** 新建 `providers::interactions_http`（TTS 请求/解析/流式 + 共用 URL/auth）；`tools` 层 Google 直连该 helper，失败不回退 OpenAI。

**Tech Stack:** Rust (`providers` / `tools`)、`reqwest`、`serde_json`、`base64`、复用 `media_http::pcm_to_wav`。

**参考:** `docs/superpowers/specs/2026-07-16-tts-interactions-api-design.md`；[Speech Generation](https://ai.google.dev/gemini-api/docs/speech-generation?hl=zh-cn)

## Global Constraints

- Google：`POST {google_native_base}/v1beta/interactions`，Header `x-goog-api-key`（不用 Bearer / `?key=`）。
- 默认模型：`gemini-3.1-flash-tts-preview`。
- Google 失败：**不**回退 OpenAI / generateContent。
- 多说话人最多 2；PCM → WAV 24kHz mono 16-bit。
- 非流式 HTTP 500：最多重试 2 次。
- OpenAI：只使用 `text` + `voice`；高级参数 → `note:`。

## File Structure

| File | Responsibility |
|------|----------------|
| `providers/src/protocol/interactions_http.rs` | 新建：TTS 类型、body、解析、流式、HTTP |
| `providers/src/protocol/mod.rs` | `pub mod interactions_http` |
| `providers/src/lib.rs` | re-export |
| `providers/src/protocol/media_http.rs` | 弃用注释 `google_tts_generate` |
| `tools/src/builtins/media/tts.rs` | Args 扩展、校验、接线 |

---

### Task 1: interactions_http TTS

**Files:** Create/Modify as above.

**Interfaces:**

- `InteractionSpeechConfig { speaker: Option<String>, voice: String }`
- `InteractionTtsRequest { model, input, speech_config, stream }`
- `InteractionTtsResult { wav_bytes, interaction_id }`
- `build_interaction_tts_body` / `parse_interaction_tts_response` / `parse_tts_stream_events`
- `google_interactions_tts`

- [x] 实现 + 单测（body、非流式解析、流式 chunk）

### Task 2: tts 工具接线

- [x] 扩展 `TtsArgs`；Google → Interactions；无 Google 才 OpenAI；note 行为

### Task 3: 弃用旧路径

- [x] `google_tts_generate` 文档标注弃用

### Task 4: 验证

- [x] `cargo test -p providers interactions_http`；tools 相关编译
