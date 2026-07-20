//! Google Gemini Interactions 聊天适配器。
//!
//! 将内部 [`ChatMessage`] / OpenAI tools 编成 `POST /v1beta/interactions`，
//! 并把 SSE（`step.delta` 等）映射回 [`ChatChunk`]。
//!
//! 首轮将完整会话历史编成 `input`；工具多轮若配置了
//! [`ProviderConfig::previous_interaction_id`]，则只提交尾部 `function_result`，
//! 由服务端保留上一轮 `thought`/`signature`（Gemini 3 严格模式必需）。

use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use reqwest::{Client, Response};
use serde_json::{json, Value};
use std::time::Duration;

use super::interactions_http::interactions_url;
use crate::http_stream::merge_additional_params;
use crate::streaming::Usage;
use crate::trait_::{
    ChatChunk, ChatContentPart, ChatMessage, ChatStream, ProviderConfig, ToolCallDeltaChunk,
};

const API_REVISION: &str = "2026-05-20";
const MAX_CONNECT_ATTEMPTS: usize = 3;

async fn send_interactions_chat_request(
    client: &Client,
    url: &str,
    api_key: &str,
    body: &Value,
) -> Result<Response> {
    let mut last_error = None;
    for attempt in 1..=MAX_CONNECT_ATTEMPTS {
        match client
            .post(url)
            .header("content-type", "application/json")
            .header("x-goog-api-key", api_key)
            .header("Api-Revision", API_REVISION)
            .json(body)
            .send()
            .await
        {
            Ok(response) => return Ok(response),
            Err(error)
                if attempt < MAX_CONNECT_ATTEMPTS && (error.is_connect() || error.is_timeout()) =>
            {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
            }
            Err(error) => return Err(error).context("发送 Google Interactions 请求失败"),
        }
    }
    Err(last_error.expect("retry loop must retain the final connection error"))
        .context("发送 Google Interactions 请求失败")
}

/// OpenAI tools 数组 → Interactions `tools`（扁平 `{type,name,description,parameters}`）。
pub fn openai_tools_to_interactions(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| {
            let f = t.get("function").unwrap_or(t);
            let name = f.get("name")?.as_str()?;
            Some(json!({
                "type": "function",
                "name": name,
                "description": f.get("description").cloned().unwrap_or(json!("")),
                "parameters": f.get("parameters").cloned().unwrap_or(json!({
                    "type": "object",
                    "properties": {}
                })),
            }))
        })
        .collect()
}

/// 从 data URL / http(s) 拼 Interactions media content 块。
fn media_content_from_url(kind: &str, url: &str, mime_hint: &str) -> Value {
    let url = url.trim();
    if let Some((mime, b64)) = crate::http_stream::parse_data_url(url) {
        return json!({
            "type": kind,
            "mime_type": if mime.is_empty() { mime_hint.to_string() } else { mime },
            "data": b64,
        });
    }
    json!({
        "type": kind,
        "uri": url,
    })
}

fn image_content_from_url(url: &str) -> Value {
    media_content_from_url("image", url, "image/png")
}

fn user_content_parts(m: &ChatMessage) -> Vec<Value> {
    if let Some(ref parts) = m.parts {
        if !parts.is_empty() {
            return parts
                .iter()
                .map(|p| match p {
                    ChatContentPart::Text { text } => json!({ "type": "text", "text": text }),
                    ChatContentPart::ImageUrl { url } => image_content_from_url(url),
                    ChatContentPart::AudioUrl { url, mime_type } => {
                        let hint = if mime_type.trim().is_empty() {
                            "audio/wav"
                        } else {
                            mime_type.as_str()
                        };
                        media_content_from_url("audio", url, hint)
                    }
                    ChatContentPart::VideoUrl { url, mime_type } => {
                        let hint = if mime_type.trim().is_empty() {
                            "video/mp4"
                        } else {
                            mime_type.as_str()
                        };
                        media_content_from_url("video", url, hint)
                    }
                })
                .collect();
        }
    }
    vec![json!({ "type": "text", "text": m.content })]
}

fn encode_function_result(m: &ChatMessage) -> Value {
    let call_id = m
        .tool_call_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    let name = m
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("tool");
    json!({
        "type": "function_result",
        "call_id": call_id,
        "name": name,
        "result": [{ "type": "text", "text": m.content }],
    })
}

/// 取消息列表尾部连续的 `tool` 角色 → Interactions `function_result` 步骤。
pub fn trailing_function_results(messages: &[ChatMessage]) -> Vec<Value> {
    let mut out = Vec::new();
    for m in messages.iter().rev() {
        if m.role == "tool" {
            out.push(encode_function_result(m));
        } else {
            break;
        }
    }
    out.reverse();
    out
}

/// 取消息列表尾部连续的 `user` 角色 → Interactions `user_input` 步骤。
///
/// 有状态多轮（`previous_interaction_id`）时只提交本轮新用户输入，
/// 由服务端保留 thought / signature。
pub fn trailing_user_inputs(messages: &[ChatMessage]) -> Vec<Value> {
    let mut out = Vec::new();
    for m in messages.iter().rev() {
        if m.role == "user" {
            out.push(json!({
                "type": "user_input",
                "content": user_content_parts(m),
            }));
        } else {
            break;
        }
    }
    out.reverse();
    out
}

/// 从消息历史抽出 `system_instruction`（多条 system 用空行拼接）。
pub fn extract_system_instruction(messages: &[ChatMessage]) -> Option<String> {
    let parts: Vec<String> = messages
        .iter()
        .filter(|m| m.role == "system")
        .map(|m| m.content.trim())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

/// 将内部消息历史编成 Interactions `input` 步骤数组，并抽出 `system_instruction`。
pub fn messages_to_interactions_input(messages: &[ChatMessage]) -> (Option<String>, Vec<Value>) {
    let mut input: Vec<Value> = Vec::new();

    for m in messages {
        match m.role.as_str() {
            "system" => {}
            "user" => {
                input.push(json!({
                    "type": "user_input",
                    "content": user_content_parts(m),
                }));
            }
            "assistant" => {
                if let Some(ref calls) = m.tool_calls {
                    if !calls.is_empty() {
                        if let Some(thought) = encode_thought_step(m) {
                            input.push(thought);
                        }
                        if !m.content.trim().is_empty() {
                            input.push(json!({
                                "type": "model_output",
                                "content": [{ "type": "text", "text": m.content }],
                            }));
                        }
                        for c in calls {
                            let mut fc = json!({
                                "type": "function_call",
                                "id": c.id,
                                "name": c.name,
                                "arguments": c.arguments,
                            });
                            if let Some(sig) = c
                                .signature
                                .as_deref()
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                            {
                                fc["signature"] = json!(sig);
                            }
                            input.push(fc);
                        }
                        continue;
                    }
                }
                input.push(json!({
                    "type": "model_output",
                    "content": [{ "type": "text", "text": m.content }],
                }));
            }
            "tool" => {
                input.push(encode_function_result(m));
            }
            _ => {
                // 未知角色当作 user
                input.push(json!({
                    "type": "user_input",
                    "content": user_content_parts(m),
                }));
            }
        }
    }

    (extract_system_instruction(messages), input)
}

/// 无状态回放：在 `function_call` 前编入 `thought` step（signature ± reasoning 文本）。
fn encode_thought_step(m: &ChatMessage) -> Option<Value> {
    let sig = m
        .thought_signature
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let reasoning = m
        .reasoning
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if sig.is_none() && reasoning.is_none() {
        return None;
    }
    let mut step = json!({ "type": "thought" });
    if let Some(s) = sig {
        step["signature"] = json!(s);
    }
    if let Some(text) = reasoning {
        step["content"] = json!([{ "type": "text", "text": text }]);
    }
    Some(step)
}

/// Gemini 3.x 默认会思考；未显式传 `thinking_level` 时，短 `max_output_tokens`
///（如标题生成的 64）会被 thought 吃光，返回 `incomplete` 且无可见文本。
/// 因此关闭思考时仍下发 `minimal`，而不是省略该字段。
fn thinking_level(config: &ProviderConfig) -> &'static str {
    if !config.thinking_enabled {
        return "minimal";
    }
    match config.reasoning_effort.trim() {
        "max" | "xhigh" | "high" => "high",
        "minimal" | "min" => "minimal",
        "low" => "low",
        _ => "medium",
    }
}

/// ListModels 返回的 id 常带 `models/` 前缀；Interactions 接受两者，但统一去掉更稳妥。
fn normalize_google_model(model: &str) -> &str {
    model.trim().strip_prefix("models/").unwrap_or(model.trim())
}

/// 拼装 Interactions 聊天请求体（默认 store=true + `stream=true`）。
///
/// 若配置了 `previous_interaction_id`：
/// - 尾部是 `tool` 结果 → 只提交增量 `function_result`（服务端保留 thought/signature）
/// - 尾部是 `user` → 只提交增量 `user_input`
/// - 否则回退全量历史（不带 previous_interaction_id）
///
/// `system_instruction` / `tools` / `generation_config` 为 interaction-scoped，有状态续写时仍重传。
pub fn build_interactions_chat_body(
    messages: &[ChatMessage],
    tools: &[Value],
    config: &ProviderConfig,
) -> Value {
    let prev = config
        .previous_interaction_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let system = extract_system_instruction(messages);
    let (input, use_prev) = if prev.is_some() {
        let results = trailing_function_results(messages);
        if !results.is_empty() {
            (results, true)
        } else {
            let users = trailing_user_inputs(messages);
            if !users.is_empty() {
                (users, true)
            } else {
                let (_, input) = messages_to_interactions_input(messages);
                (input, false)
            }
        }
    } else {
        let (_, input) = messages_to_interactions_input(messages);
        (input, false)
    };

    let mut body = json!({
        "model": normalize_google_model(&config.model),
        "input": input,
        "stream": true,
    });
    if use_prev {
        if let Some(prev_id) = prev {
            body["previous_interaction_id"] = json!(prev_id);
        }
    }
    if let Some(sys) = system {
        body["system_instruction"] = json!(sys);
    }
    let ix_tools = openai_tools_to_interactions(tools);
    if !ix_tools.is_empty() {
        body["tools"] = Value::Array(ix_tools);
    }

    let mut gen = serde_json::Map::new();
    // Gemini 3.5 文档建议去掉 temperature；保留 max_tokens 若上游支持
    if config.max_tokens > 0 {
        gen.insert("max_output_tokens".into(), json!(config.max_tokens));
    }
    gen.insert("thinking_level".into(), json!(thinking_level(config)));
    if !gen.is_empty() {
        body["generation_config"] = Value::Object(gen);
    }

    merge_additional_params(&mut body, &config.additional_params);
    body
}

fn parse_usage(v: &Value) -> Option<Usage> {
    let u = v.pointer("/interaction/usage").or_else(|| v.get("usage"))?;
    if u.is_null() {
        return None;
    }
    // 线上 Interactions：`total_input_tokens` / `total_output_tokens` /
    // `total_thought_tokens` / `total_cached_tokens`；兼容旧 prompt_/input_ 命名。
    let prompt = u
        .get("total_input_tokens")
        .or_else(|| u.get("prompt_tokens"))
        .or_else(|| u.get("input_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    let output = u
        .get("total_output_tokens")
        .or_else(|| u.get("completion_tokens"))
        .or_else(|| u.get("output_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    let reasoning = u
        .get("total_thought_tokens")
        .or_else(|| u.get("reasoning_tokens"))
        .or_else(|| u.pointer("/completion_tokens_details/reasoning_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    let cache_read = u
        .get("total_cached_tokens")
        .or_else(|| u.get("cached_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    if prompt == 0 && output == 0 && reasoning == 0 && cache_read == 0 {
        return None;
    }
    Some(Usage {
        input_tokens: prompt,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: 0,
        reasoning_tokens: reasoning,
        request_count: 1,
    })
}

/// 根据 interaction.status / saw_function_call 映射 finish_reason。
fn interaction_finish_reason(v: &Value, saw_function_call: bool) -> String {
    let status = v
        .pointer("/interaction/status")
        .or_else(|| v.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("");
    match status {
        "requires_action" => "tool_calls".into(),
        "failed" | "cancelled" => {
            let msg = v
                .pointer("/interaction/error/message")
                .or_else(|| v.pointer("/error/message"))
                .and_then(|m| m.as_str())
                .unwrap_or(if status == "cancelled" {
                    "Interactions 已取消"
                } else {
                    "Interactions 失败"
                });
            format!("error:{msg}")
        }
        "incomplete" => "incomplete".into(),
        _ => {
            if saw_function_call {
                "tool_calls".into()
            } else {
                "stop".into()
            }
        }
    }
}

fn extract_interaction_id(v: &Value) -> Option<String> {
    v.pointer("/interaction/id")
        .or_else(|| v.get("id"))
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 将 step 上的 arguments 转为非空 JSON 字符串；空对象/空串返回 None，避免污染增量拼接。
fn meaningful_arguments_json(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() || t == "{}" || t == "null" {
                None
            } else {
                Some(s.clone())
            }
        }
        Value::Object(m) if m.is_empty() => None,
        other => Some(other.to_string()),
    }
}

/// 流式解析状态：跟踪当前 function_call step 的 index → tool_call 槽位。
#[derive(Default)]
pub struct StreamState {
    /// step.index → 分配给 ChatChunk 的 tool_call index
    fc_slots: Vec<(u64, u32)>,
    next_fc_index: u32,
    saw_function_call: bool,
    /// 最近一个 `function_call` step.start 分配到的槽位。
    ///
    /// `arguments` 增量事件的顶层 `index` 可能缺失或与 function_call step 的
    /// index 不一致（Gemini 3 默认先产出 `thought` step，占用 index 0），
    /// 此时参数应归到当前 function_call，而非新建一个无名槽位。
    current_fc_slot: Option<u32>,
    interaction_id: Option<String>,
}

impl StreamState {
    fn slot_for(&mut self, step_index: u64) -> u32 {
        if let Some((_, slot)) = self.fc_slots.iter().find(|(i, _)| *i == step_index) {
            return *slot;
        }
        let slot = self.next_fc_index;
        self.next_fc_index += 1;
        self.fc_slots.push((step_index, slot));
        slot
    }

    /// 仅返回已登记的 step.index 对应槽位，不新建。
    fn existing_slot(&self, step_index: u64) -> Option<u32> {
        self.fc_slots
            .iter()
            .find(|(i, _)| *i == step_index)
            .map(|(_, s)| *s)
    }

    /// 为一次 `arguments` 增量解析目标槽位：优先按 index 命中已知
    /// function_call，其次归到当前 function_call，绝不新建无名槽位。
    fn arguments_slot(&self, step_index: Option<u64>) -> Option<u32> {
        step_index
            .and_then(|i| self.existing_slot(i))
            .or(self.current_fc_slot)
    }

    fn note_interaction_id(&mut self, v: &Value) {
        if let Some(id) = extract_interaction_id(v) {
            self.interaction_id = Some(id);
        }
    }
}

/// 将单条 SSE JSON 负载转为零或多个 [`ChatChunk`]（公开以便单测）。
pub fn extract_interactions_chat_events(data: &str, state: &mut StreamState) -> Vec<ChatChunk> {
    let Ok(v) = serde_json::from_str::<Value>(data) else {
        return Vec::new();
    };
    extract_interactions_chat_value(&v, state)
}

fn event_type(v: &Value) -> &str {
    v.get("event_type")
        .or_else(|| v.get("type"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
}

fn extract_interactions_chat_value(v: &Value, state: &mut StreamState) -> Vec<ChatChunk> {
    let et = event_type(v);
    let mut out = Vec::new();

    state.note_interaction_id(v);

    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .or_else(|| err.as_str())
            .unwrap_or("上游 Interactions 错误");
        out.push(ChatChunk {
            finish_reason: Some(format!("error:{msg}")),
            interaction_id: state.interaction_id.clone(),
            ..Default::default()
        });
        return out;
    }

    match et {
        "interaction.created" | "interaction.in_progress" => {
            // 仅记录 interaction_id，无可见内容
        }
        "step.start" => {
            let step = v.get("step");
            let step_type = step
                .and_then(|s| s.get("type"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if step_type == "function_call" {
                state.saw_function_call = true;
                let step_index = v
                    .get("index")
                    .and_then(|i| i.as_u64())
                    .unwrap_or(state.next_fc_index as u64);
                let slot = state.slot_for(step_index);
                state.current_fc_slot = Some(slot);
                let id = step
                    .and_then(|s| s.get("id"))
                    .and_then(|x| x.as_str())
                    .map(str::to_string);
                let name = step
                    .and_then(|s| s.get("name"))
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                let signature = step
                    .and_then(|s| s.get("signature"))
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                // 完整 arguments（非流式增量时可能直接出现在 step 上）；跳过空对象
                let arguments = step
                    .and_then(|s| s.get("arguments"))
                    .and_then(meaningful_arguments_json);
                out.push(ChatChunk {
                    tool_call_deltas: vec![ToolCallDeltaChunk {
                        index: slot,
                        id,
                        name,
                        arguments,
                        signature,
                    }],
                    interaction_id: state.interaction_id.clone(),
                    ..Default::default()
                });
            } else if step_type == "thought" {
                let signature = step
                    .and_then(|s| s.get("signature"))
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                if let Some(signature) = signature {
                    out.push(ChatChunk {
                        thought_signature: Some(signature),
                        interaction_id: state.interaction_id.clone(),
                        ..Default::default()
                    });
                }
            }
        }
        "step.delta" => {
            let delta = v.get("delta");
            let dtype = delta
                .and_then(|d| d.get("type"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            match dtype {
                "text" => {
                    if let Some(text) = delta
                        .and_then(|d| d.get("text"))
                        .and_then(|t| t.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        out.push(ChatChunk {
                            token: Some(text.to_string()),
                            interaction_id: state.interaction_id.clone(),
                            ..Default::default()
                        });
                    }
                }
                "thought" | "thought_summary" => {
                    let text = delta
                        .and_then(|d| d.get("text"))
                        .or_else(|| delta.and_then(|d| d.get("summary")))
                        .and_then(|t| t.as_str())
                        .filter(|s| !s.is_empty());
                    if let Some(text) = text {
                        out.push(ChatChunk {
                            reasoning: Some(text.to_string()),
                            interaction_id: state.interaction_id.clone(),
                            ..Default::default()
                        });
                    }
                }
                "thought_signature" => {
                    let signature = delta
                        .and_then(|d| d.get("signature"))
                        .and_then(|t| t.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    if let Some(signature) = signature {
                        out.push(ChatChunk {
                            thought_signature: Some(signature),
                            interaction_id: state.interaction_id.clone(),
                            ..Default::default()
                        });
                    }
                }
                // 线上 Interactions SSE：`type=arguments_delta` + `arguments` 字符串增量。
                // 文档示例仍写 `type=arguments` + `partial_arguments`；两者都认。
                "arguments" | "arguments_delta" => {
                    state.saw_function_call = true;
                    // 参数增量归到当前 function_call 槽位；顶层 index 缺失或与
                    // function_call step 的 index 不一致时也不会拆散 name/arguments。
                    let step_index = v.get("index").and_then(|i| i.as_u64());
                    let slot = state.arguments_slot(step_index).unwrap_or(0);
                    let partial = delta
                        .and_then(|d| {
                            d.get("partial_arguments")
                                .or_else(|| d.get("arguments"))
                                .or_else(|| d.get("text"))
                        })
                        .and_then(meaningful_arguments_json);
                    if let Some(arguments) = partial {
                        out.push(ChatChunk {
                            tool_call_deltas: vec![ToolCallDeltaChunk {
                                index: slot,
                                id: None,
                                name: None,
                                arguments: Some(arguments),
                                signature: None,
                            }],
                            interaction_id: state.interaction_id.clone(),
                            ..Default::default()
                        });
                    }
                }
                _ => {}
            }
        }
        "interaction.requires_action" => {
            state.saw_function_call = true;
            out.push(ChatChunk {
                finish_reason: Some(interaction_finish_reason(v, true)),
                usage: parse_usage(v),
                interaction_id: state.interaction_id.clone(),
                ..Default::default()
            });
        }
        "interaction.completed" => {
            let status = v
                .pointer("/interaction/status")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            if status == "requires_action" {
                state.saw_function_call = true;
            }
            out.push(ChatChunk {
                finish_reason: Some(interaction_finish_reason(v, state.saw_function_call)),
                usage: parse_usage(v),
                interaction_id: state.interaction_id.clone(),
                ..Default::default()
            });
        }
        "interaction.failed" | "interaction.error" | "error" => {
            let msg = v
                .pointer("/interaction/error/message")
                .or_else(|| v.pointer("/error/message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Interactions 失败");
            out.push(ChatChunk {
                finish_reason: Some(format!("error:{msg}")),
                interaction_id: state.interaction_id.clone(),
                ..Default::default()
            });
        }
        _ => {}
    }

    out
}

/// Google Interactions 流式聊天。
pub async fn interactions_chat_stream(
    client: &Client,
    messages: Vec<ChatMessage>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> Result<ChatStream> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let url = interactions_url(config);
    let body = build_interactions_chat_body(&messages, &tools, config);

    let response = send_interactions_chat_request(client, &url, config.api_key.trim(), &body)
        .await
        .with_context(|| format!("连接 Google Interactions 失败: {url}"))?;

    let response = crate::http_stream::check_response_status(response).await?;
    let byte_stream = response.bytes_stream();
    let stream = futures::stream::unfold(
        (byte_stream, String::new(), false, StreamState::default()),
        |(mut byte_stream, mut buf, done, mut state)| async move {
            if done {
                return None;
            }
            loop {
                if let Some(nl) = buf.find('\n') {
                    let line = buf[..nl].trim_end_matches('\r').to_string();
                    buf = buf[nl + 1..].to_string();
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with(':') {
                        continue;
                    }
                    // 忽略 SSE event: 行，只处理 data:
                    if !trimmed.starts_with("data:") {
                        continue;
                    }
                    let data = trimmed.strip_prefix("data:").unwrap_or("").trim();
                    if data == "[DONE]" {
                        return None;
                    }
                    let chunks = extract_interactions_chat_events(data, &mut state);
                    for chunk in &chunks {
                        if chunk
                            .finish_reason
                            .as_deref()
                            .is_some_and(|f| f.starts_with("error:"))
                        {
                            let msg = chunk
                                .finish_reason
                                .as_deref()
                                .unwrap_or("error")
                                .trim_start_matches("error:")
                                .to_string();
                            return Some((Err(anyhow!(msg)), (byte_stream, buf, true, state)));
                        }
                    }
                    if !chunks.is_empty() {
                        let merged = merge_chunks(chunks);
                        return Some((Ok(merged), (byte_stream, buf, false, state)));
                    }
                    continue;
                }

                match byte_stream.next().await {
                    Some(Ok(bytes)) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Some(Err(err)) => {
                        return Some((Err(err.into()), (byte_stream, buf, true, state)));
                    }
                    None => {
                        if !buf.trim().is_empty() {
                            let line = buf.trim().to_string();
                            buf.clear();
                            if let Some(data) = line.strip_prefix("data:") {
                                let data = data.trim();
                                if data != "[DONE]" {
                                    let chunks = extract_interactions_chat_events(data, &mut state);
                                    if !chunks.is_empty() {
                                        return Some((
                                            Ok(merge_chunks(chunks)),
                                            (byte_stream, buf, true, state),
                                        ));
                                    }
                                }
                            }
                        }
                        return None;
                    }
                }
            }
        },
    );

    Ok(Box::pin(stream))
}

fn merge_chunks(chunks: Vec<ChatChunk>) -> ChatChunk {
    if chunks.len() == 1 {
        return chunks.into_iter().next().unwrap();
    }
    let mut out = ChatChunk::default();
    for c in chunks {
        if let Some(t) = c.token {
            out.token = Some(match out.token.take() {
                Some(prev) => prev + &t,
                None => t,
            });
        }
        if let Some(r) = c.reasoning {
            out.reasoning = Some(match out.reasoning.take() {
                Some(prev) => prev + &r,
                None => r,
            });
        }
        out.tool_call_deltas.extend(c.tool_call_deltas);
        if c.finish_reason.is_some() {
            out.finish_reason = c.finish_reason;
        }
        if c.usage.is_some() {
            out.usage = c.usage;
        }
        if c.interaction_id.is_some() {
            out.interaction_id = c.interaction_id;
        }
    }
    out
}

/// Interactions 连通性探测（非流式最小请求）。
pub async fn probe_interactions(
    client: &Client,
    model: &str,
    config: &ProviderConfig,
) -> Result<String, String> {
    if config.api_key.trim().is_empty() {
        return Err("Google API Key 为空".into());
    }
    let url = interactions_url(config);
    let body = json!({
        "model": model,
        "input": "ping",
        "generation_config": { "max_output_tokens": 1 }
    });
    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .header("x-goog-api-key", config.api_key.trim())
        .header("Api-Revision", API_REVISION)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("连接 Google Interactions 失败: {e}"))?;
    let status = resp.status();
    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let msg = json
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        return Err(format!("失败 ({status}): {msg}"));
    }
    Ok("调用成功".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trait_::ChatToolCall;

    #[test]
    fn tools_conversion_flattens_openai_function() {
        let tools = vec![json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "weather",
                "parameters": {
                    "type": "object",
                    "properties": { "location": { "type": "string" } },
                    "required": ["location"]
                }
            }
        })];
        let out = openai_tools_to_interactions(&tools);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["type"], "function");
        assert_eq!(out[0]["name"], "get_weather");
        assert_eq!(out[0]["parameters"]["required"][0], "location");
    }

    #[test]
    fn messages_encode_system_user_and_tools() {
        let messages = vec![
            ChatMessage::text("system", "You are helpful."),
            ChatMessage::text("user", "weather in Boston?"),
            ChatMessage {
                role: "assistant".into(),
                content: String::new(),
                parts: None,
                tool_calls: Some(vec![ChatToolCall {
                    id: "fc_1".into(),
                    name: "get_weather".into(),
                    arguments: json!({ "location": "Boston" }),
                    signature: None,
                }]),
                tool_call_id: None,
                name: None,
                reasoning: None,
                thought_signature: None,
            },
            ChatMessage {
                role: "tool".into(),
                content: "52F".into(),
                parts: None,
                tool_calls: None,
                tool_call_id: Some("fc_1".into()),
                name: Some("get_weather".into()),
                reasoning: None,
                thought_signature: None,
            },
            ChatMessage::text("user", "thanks"),
        ];
        let (sys, input) = messages_to_interactions_input(&messages);
        assert_eq!(sys.as_deref(), Some("You are helpful."));
        assert_eq!(input[0]["type"], "user_input");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["id"], "fc_1");
        assert_eq!(input[2]["type"], "function_result");
        assert_eq!(input[2]["call_id"], "fc_1");
        assert_eq!(input[3]["type"], "user_input");
    }

    #[test]
    fn stream_text_and_thought_deltas() {
        let mut state = StreamState::default();
        let chunks = extract_interactions_chat_events(
            r#"{"event_type":"step.delta","index":1,"delta":{"type":"text","text":"Hi"}}"#,
            &mut state,
        );
        assert_eq!(chunks[0].token.as_deref(), Some("Hi"));

        let chunks = extract_interactions_chat_events(
            r#"{"type":"step.delta","index":0,"delta":{"type":"thought","text":"think"}}"#,
            &mut state,
        );
        assert_eq!(chunks[0].reasoning.as_deref(), Some("think"));
    }

    #[test]
    fn stream_function_call_lifecycle() {
        let mut state = StreamState::default();
        let start = extract_interactions_chat_events(
            r#"{"event_type":"step.start","index":1,"step":{"type":"function_call","id":"fc_1","name":"get_weather"}}"#,
            &mut state,
        );
        assert_eq!(start[0].tool_call_deltas[0].id.as_deref(), Some("fc_1"));
        assert_eq!(
            start[0].tool_call_deltas[0].name.as_deref(),
            Some("get_weather")
        );
        assert!(start[0].tool_call_deltas[0].arguments.is_none());

        let args = extract_interactions_chat_events(
            r#"{"event_type":"step.delta","index":1,"delta":{"type":"arguments","partial_arguments":"{\"location\":\"Boston\"}"}}"#,
            &mut state,
        );
        assert_eq!(
            args[0].tool_call_deltas[0].arguments.as_deref(),
            Some(r#"{"location":"Boston"}"#)
        );

        let done = extract_interactions_chat_events(
            r#"{"event_type":"interaction.requires_action","interaction":{"id":"ix_1","status":"requires_action"}}"#,
            &mut state,
        );
        assert_eq!(done[0].finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(done[0].interaction_id.as_deref(), Some("ix_1"));
    }

    #[test]
    fn stream_arguments_delta_wire_format() {
        // 真实线上 SSE（Api-Revision 2026-05-20）：
        // step.start 带空 arguments:{}；参数在 step.delta type=arguments_delta
        // 的 `arguments` 字段里，而不是文档示例的 type=arguments/partial_arguments。
        let mut state = StreamState::default();
        let start = extract_interactions_chat_events(
            r#"{"index":0,"step":{"id":"sf9vftls","type":"function_call","name":"image_gen","arguments":{},"signature":"sig_wire"},"event_type":"step.start"}"#,
            &mut state,
        );
        assert_eq!(
            start[0].tool_call_deltas[0].name.as_deref(),
            Some("image_gen")
        );
        assert_eq!(
            start[0].tool_call_deltas[0].signature.as_deref(),
            Some("sig_wire")
        );
        assert!(start[0].tool_call_deltas[0].arguments.is_none());
        let name_slot = start[0].tool_call_deltas[0].index;

        let args = extract_interactions_chat_events(
            r#"{"index":0,"delta":{"arguments":"{\"prompt\":\"a cat\"}","type":"arguments_delta"},"event_type":"step.delta"}"#,
            &mut state,
        );
        assert_eq!(args[0].tool_call_deltas[0].index, name_slot);
        assert_eq!(
            args[0].tool_call_deltas[0].arguments.as_deref(),
            Some(r#"{"prompt":"a cat"}"#)
        );

        let done = extract_interactions_chat_events(
            r#"{"interaction":{"id":"ix_wire","status":"requires_action","usage":{"total_input_tokens":10,"total_output_tokens":5,"total_thought_tokens":3,"total_cached_tokens":2}},"event_type":"interaction.completed"}"#,
            &mut state,
        );
        assert_eq!(done[0].finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(done[0].interaction_id.as_deref(), Some("ix_wire"));
        let usage = done[0].usage.as_ref().expect("usage from total_* fields");
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.reasoning_tokens, 3);
        assert_eq!(usage.cache_read_tokens, 2);
    }

    #[test]
    fn arguments_delta_without_index_binds_to_current_function_call() {
        // Gemini 3 先产出 thought(step index 0)，function_call 在 index 1；
        // 若 arguments 增量缺失顶层 index（回退 0），旧逻辑会把参数分到一个
        // 新的无名槽位，导致工具入参丢成 `{}`。此处校验参数仍归到 name 的槽位。
        let mut state = StreamState::default();

        let _thought = extract_interactions_chat_events(
            r#"{"event_type":"step.start","index":0,"step":{"type":"thought"}}"#,
            &mut state,
        );
        let start = extract_interactions_chat_events(
            r#"{"event_type":"step.start","index":1,"step":{"type":"function_call","id":"fc_1","name":"image_gen"}}"#,
            &mut state,
        );
        let name_slot = start[0].tool_call_deltas[0].index;
        assert_eq!(
            start[0].tool_call_deltas[0].name.as_deref(),
            Some("image_gen")
        );

        // arguments 增量不带顶层 index
        let args = extract_interactions_chat_events(
            r#"{"event_type":"step.delta","delta":{"type":"arguments","partial_arguments":"{\"prompt\":\"a cat\"}"}}"#,
            &mut state,
        );
        assert_eq!(args[0].tool_call_deltas[0].index, name_slot);
        assert_eq!(
            args[0].tool_call_deltas[0].arguments.as_deref(),
            Some(r#"{"prompt":"a cat"}"#)
        );

        // arguments 增量带 thought 的 index 0（与 function_call 的 index 1 不一致）
        let args2 = extract_interactions_chat_events(
            r#"{"event_type":"step.delta","index":0,"delta":{"type":"arguments","partial_arguments":"!"}}"#,
            &mut state,
        );
        assert_eq!(args2[0].tool_call_deltas[0].index, name_slot);
    }

    #[test]
    fn step_start_skips_empty_arguments_object() {
        let mut state = StreamState::default();
        let start = extract_interactions_chat_events(
            r#"{"event_type":"step.start","index":0,"step":{"type":"function_call","id":"fc_1","name":"f","arguments":{}}}"#,
            &mut state,
        );
        assert!(start[0].tool_call_deltas[0].arguments.is_none());
    }

    #[test]
    fn interaction_error_maps_to_finish_reason() {
        let mut state = StreamState::default();
        let chunks = extract_interactions_chat_events(
            r#"{"event_type":"interaction.error","interaction":{"id":"ix","error":{"message":"boom"}}}"#,
            &mut state,
        );
        assert_eq!(chunks[0].finish_reason.as_deref(), Some("error:boom"));
        assert_eq!(chunks[0].interaction_id.as_deref(), Some("ix"));
    }

    #[test]
    fn build_body_sets_thinking_and_defaults_store() {
        let messages = vec![ChatMessage::text("user", "hi")];
        let config = ProviderConfig {
            model: "models/gemini-3.5-flash".into(),
            thinking_enabled: true,
            reasoning_effort: "high".into(),
            ..Default::default()
        };
        let body = build_interactions_chat_body(&messages, &[], &config);
        assert_eq!(body["stream"], true);
        assert_eq!(body["model"], "gemini-3.5-flash");
        assert!(body.get("store").is_none() || body["store"] == true);
        assert!(body.get("previous_interaction_id").is_none());
        assert_eq!(body["generation_config"]["thinking_level"], "high");
    }

    #[test]
    fn build_body_disables_thinking_with_minimal_level() {
        let messages = vec![ChatMessage::text("user", "hi")];
        let config = ProviderConfig {
            model: "gemini-3.5-flash".into(),
            thinking_enabled: false,
            max_tokens: 64,
            ..Default::default()
        };
        let body = build_interactions_chat_body(&messages, &[], &config);
        assert_eq!(body["generation_config"]["thinking_level"], "minimal");
        assert_eq!(body["generation_config"]["max_output_tokens"], 64);
    }

    #[test]
    fn messages_replay_function_call_signature() {
        let messages = vec![ChatMessage {
            role: "assistant".into(),
            content: String::new(),
            parts: None,
            tool_calls: Some(vec![ChatToolCall {
                id: "fc_1".into(),
                name: "get_weather".into(),
                arguments: json!({ "location": "Paris" }),
                signature: Some("sig_paris".into()),
            }]),
            tool_call_id: None,
            name: None,
            reasoning: None,
            thought_signature: None,
        }];
        let (_, input) = messages_to_interactions_input(&messages);
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["signature"], "sig_paris");
        assert_eq!(input[0]["arguments"]["location"], "Paris");
    }

    #[test]
    fn messages_replay_thought_before_function_call() {
        let messages = vec![ChatMessage {
            role: "assistant".into(),
            content: String::new(),
            parts: None,
            tool_calls: Some(vec![ChatToolCall {
                id: "fc_1".into(),
                name: "image_gen".into(),
                arguments: json!({ "prompt": "cat" }),
                signature: Some("fc_sig".into()),
            }]),
            tool_call_id: None,
            name: None,
            reasoning: Some("plan image".into()),
            thought_signature: Some("thought_sig".into()),
        }];
        let (_, input) = messages_to_interactions_input(&messages);
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["type"], "thought");
        assert_eq!(input[0]["signature"], "thought_sig");
        assert_eq!(input[0]["content"][0]["text"], "plan image");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["signature"], "fc_sig");
    }

    #[test]
    fn stream_thought_signature_delta() {
        let mut state = StreamState::default();
        let chunks = extract_interactions_chat_events(
            r#"{"event_type":"step.delta","index":0,"delta":{"type":"thought_signature","signature":"EvEFCu4F"}}"#,
            &mut state,
        );
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].thought_signature.as_deref(), Some("EvEFCu4F"));
    }

    #[test]
    fn stream_thought_step_start_signature() {
        let mut state = StreamState::default();
        let chunks = extract_interactions_chat_events(
            r#"{"event_type":"step.start","index":0,"step":{"type":"thought","signature":"sig_thought"}}"#,
            &mut state,
        );
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].thought_signature.as_deref(), Some("sig_thought"));
    }

    #[test]
    fn build_body_with_previous_interaction_sends_only_tool_results() {
        let messages = vec![
            ChatMessage::text("system", "Be helpful."),
            ChatMessage::text("user", "weather?"),
            ChatMessage {
                role: "assistant".into(),
                content: String::new(),
                parts: None,
                tool_calls: Some(vec![ChatToolCall {
                    id: "fc_1".into(),
                    name: "get_weather".into(),
                    arguments: json!({ "location": "Boston" }),
                    signature: None,
                }]),
                tool_call_id: None,
                name: None,
                reasoning: None,
                thought_signature: None,
            },
            ChatMessage {
                role: "tool".into(),
                content: "52F".into(),
                parts: None,
                tool_calls: None,
                tool_call_id: Some("fc_1".into()),
                name: Some("get_weather".into()),
                reasoning: None,
                thought_signature: None,
            },
        ];
        let config = ProviderConfig {
            model: "gemini-3.5-flash".into(),
            previous_interaction_id: Some("ix_prev".into()),
            ..Default::default()
        };
        let body = build_interactions_chat_body(&messages, &[], &config);
        assert_eq!(body["previous_interaction_id"], "ix_prev");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "function_result");
        assert_eq!(input[0]["call_id"], "fc_1");
        // interaction-scoped：有状态 tool 续写仍重传 system
        assert_eq!(body["system_instruction"], "Be helpful.");
    }

    #[test]
    fn build_body_with_previous_interaction_sends_only_trailing_user() {
        let messages = vec![
            ChatMessage::text("system", "Be concise."),
            ChatMessage::text("user", "hi"),
            ChatMessage::text("assistant", "hello"),
            ChatMessage::text("user", "what about Paris?"),
        ];
        let config = ProviderConfig {
            model: "gemini-3.5-flash".into(),
            previous_interaction_id: Some("ix_prev".into()),
            ..Default::default()
        };
        let body = build_interactions_chat_body(&messages, &[], &config);
        assert_eq!(body["previous_interaction_id"], "ix_prev");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "user_input");
        assert_eq!(input[0]["content"][0]["text"], "what about Paris?");
        assert_eq!(body["system_instruction"], "Be concise.");
    }

    #[test]
    fn interaction_completed_failed_status_is_error() {
        let mut state = StreamState::default();
        let chunks = extract_interactions_chat_events(
            r#"{"event_type":"interaction.completed","interaction":{"id":"ix","status":"failed","error":{"message":"quota"}}}"#,
            &mut state,
        );
        assert_eq!(chunks[0].finish_reason.as_deref(), Some("error:quota"));
    }

    #[test]
    fn trailing_function_results_stops_at_non_tool() {
        let messages = vec![
            ChatMessage {
                role: "tool".into(),
                content: "old".into(),
                parts: None,
                tool_calls: None,
                tool_call_id: Some("a".into()),
                name: Some("t".into()),
                reasoning: None,
                thought_signature: None,
            },
            ChatMessage::text("user", "again"),
            ChatMessage {
                role: "tool".into(),
                content: "new".into(),
                parts: None,
                tool_calls: None,
                tool_call_id: Some("b".into()),
                name: Some("t".into()),
                reasoning: None,
                thought_signature: None,
            },
        ];
        let results = trailing_function_results(&messages);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["call_id"], "b");
    }
}
