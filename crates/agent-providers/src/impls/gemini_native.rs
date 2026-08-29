//! Google Gemini Native `streamGenerateContent` — 原生 CompletionModel 实现。
//!
//! 与 Interactions API 不同，此协议使用 `POST /v1beta/models/{model}:streamGenerateContent`，
//! 支持 `function_declarations` 工具调用和 `thinkingConfig`。

use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client as HttpClient;
use serde_json::{json, Value};

use crate::traits::{
    Capabilities, Capable, CompletionModel, FromClient, Nothing, ProviderClient, ProviderExt,
};
use crate::types::{CompletionRequest, CompletionStream};

// ─── Provider Extension ─────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
pub struct GeminiNative;

impl ProviderExt for GeminiNative {
    const NAME: &'static str = "gemini-native";
    const BASE_URL: &'static str = "https://generativelanguage.googleapis.com";

    fn auth_headers(&self, key: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Ok(v) = HeaderValue::from_str(key) {
            h.insert("x-goog-api-key", v);
        }
        h
    }
}

impl Capabilities for GeminiNative {
    type Chat = Capable<GeminiNativeCompletionModel>;
    type Embedding = Nothing;
    type ImageGen = Nothing;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
}

// ─── Completion Model ────────────────────────────────────

pub struct GeminiNativeCompletionModel {
    http: HttpClient,
    base_url: String,
    api_key: String,
    model: String,
}

impl Clone for GeminiNativeCompletionModel {
    fn clone(&self) -> Self {
        Self {
            http: self.http.clone(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            model: self.model.clone(),
        }
    }
}

impl FromClient<GeminiNative> for GeminiNativeCompletionModel {
    fn from_client(client: &ProviderClient<GeminiNative>, model: &str) -> Self {
        Self {
            http: client.http.clone(),
            base_url: client.base_url.clone(),
            api_key: client.api_key.clone(),
            model: model.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl CompletionModel for GeminiNativeCompletionModel {
    async fn stream(&self, request: CompletionRequest) -> Result<CompletionStream> {
        let model = if request.model.is_empty() {
            &self.model
        } else {
            &request.model
        };
        let base = self.base_url.trim_end_matches('/');
        let base = if base.contains("/v1beta") {
            base.to_string()
        } else {
            format!("{base}/v1beta")
        };
        let url = format!("{base}/models/{model}:streamGenerateContent?alt=sse");

        let messages = request.input_with_instructions();
        let (system_instruction, contents) = to_native_contents(&messages);

        let mut body = json!({ "contents": contents });

        if let Some(sys) = system_instruction {
            body["system_instruction"] = json!({
                "parts": [{"text": sys}]
            });
        }

        // 工具定义 → function_declarations
        if !request.tools.is_empty() {
            let function_tools = request
                .tools
                .iter()
                .flat_map(|tool| tool.function_definitions())
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                        }
                    })
                })
                .collect::<Vec<_>>();
            let tools = crate::google::tools::openai_tools_to_gemini_native(&function_tools);
            if !tools.is_empty() {
                body["tools"] = Value::Array(tools);
            }
        }

        // 生成参数
        let mut gen = serde_json::Map::new();
        if let Some(temp) = request.temperature {
            gen.insert("temperature".into(), json!(temp));
        }
        if let Some(max) = request.max_tokens {
            if max > 0 {
                gen.insert("maxOutputTokens".into(), json!(max));
            }
        }

        // 推理配置
        if let Some(ref tc) = request.thinking {
            let budget = if tc.enabled {
                match tc.effort.trim() {
                    "max" => 32768,
                    "high" | "xhigh" => 10240,
                    "medium" | "" => 4096,
                    "low" => 1024,
                    "minimal" | "min" => 0,
                    other => other.parse::<i32>().unwrap_or(4096),
                }
            } else {
                0
            };
            gen.insert("thinkingConfig".into(), json!({"thinkingBudget": budget}));
        }

        if !gen.is_empty() {
            body["generationConfig"] = Value::Object(gen);
        }

        // 额外参数合并
        if let Some(extra) = request.additional_params.as_object() {
            if let Some(obj) = body.as_object_mut() {
                for (k, v) in extra {
                    if !obj.contains_key(k) {
                        obj.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        crate::shared::tool_policy::apply_gemini_native(&mut body, request.tool_choice.as_ref());

        let auth = GeminiNative.auth_headers(&self.api_key);
        let response = self
            .http
            .post(&url)
            .headers(auth)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .with_context(|| format!("连接 Gemini Native 失败: {url}"))?;

        crate::shared::sse::sse_stream(response, std::sync::Arc::new(extract_native_chunks)).await
    }
}

// ─── 消息转换 ──────────────────────────────────

fn to_native_contents(messages: &[crate::types::Message]) -> (Option<String>, Vec<Value>) {
    use crate::types::message::*;
    let mut instruction_parts = Vec::new();
    let mut contents = Vec::new();

    for m in messages {
        match m {
            // Gemini 只有一个 system_instruction 字段；developer 角色降级到此字段。
            Message::System { content } | Message::Developer { content } => {
                if !content.trim().is_empty() {
                    instruction_parts.push(content.clone());
                }
            }
            Message::User { content } => {
                let parts: Vec<Value> = content
                    .iter()
                    .map(|c| match c {
                        UserContent::Text { text } => json!({"text": text}),
                        UserContent::Image { url } => inline_or_file_data(url, "image/jpeg"),
                        UserContent::Audio { url, mime_type } => {
                            inline_or_file_data(url, mime_type)
                        }
                        UserContent::Video { url, mime_type } => {
                            inline_or_file_data(url, mime_type)
                        }
                        UserContent::Document { .. } => json!({"text": "[document]"}),
                        UserContent::ToolResult { .. } => json!({"text": ""}),
                    })
                    .collect();
                contents.push(json!({"role": "user", "parts": parts}));
            }
            Message::Tool {
                tool_call_id,
                content,
                ..
            } => {
                let response_value: Value =
                    serde_json::from_str(content).unwrap_or_else(|_| json!({"result": content}));
                contents.push(json!({
                    "role": "model",
                    "parts": [{
                        "functionResponse": {
                            "name": tool_call_id,
                            "response": response_value,
                        }
                    }]
                }));
            }
            Message::Assistant { content } => {
                let mut parts = Vec::new();
                for c in content {
                    match c {
                        AssistantContent::Text { text } => {
                            if !text.is_empty() {
                                parts.push(json!({"text": text}));
                            }
                        }
                        AssistantContent::ToolCall(tc) => {
                            let args = if tc.arguments.is_string() {
                                serde_json::from_str(tc.arguments.as_str().unwrap_or("{}"))
                                    .unwrap_or(json!({}))
                            } else {
                                tc.arguments.clone()
                            };
                            parts.push(json!({
                                "functionCall": {
                                    "name": tc.name,
                                    "args": args,
                                }
                            }));
                        }
                        AssistantContent::Thinking { text, .. } => {
                            if !text.is_empty() {
                                parts.push(json!({"thought": true, "text": text}));
                            }
                        }
                    }
                }
                if parts.is_empty() {
                    parts.push(json!({"text": ""}));
                }
                contents.push(json!({"role": "model", "parts": parts}));
            }
        }
    }
    let system = (!instruction_parts.is_empty()).then(|| instruction_parts.join("\n\n"));
    (system, contents)
}

fn inline_or_file_data(url: &str, mime_hint: &str) -> Value {
    if url.starts_with("data:") {
        if let Some(rest) = url.strip_prefix("data:") {
            if let Some((meta, b64)) = rest.split_once(";base64,") {
                return json!({"inlineData": {"mimeType": meta, "data": b64}});
            }
        }
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return json!({"fileData": {"mimeType": mime_hint, "fileUri": url}});
    }
    json!({"text": format!("[media: {url}]")})
}

// ─── SSE 解析 ─────────────────────────────────────────

fn extract_native_chunks(data: &str) -> Vec<crate::types::StreamChunk> {
    use crate::types::stream::{StreamChunk, Usage};
    let Some(v) = serde_json::from_str::<Value>(data).ok() else {
        return Vec::new();
    };

    // 错误
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Gemini Native error");
        return vec![StreamChunk::Error(msg.to_string())];
    }

    let mut chunks = Vec::with_capacity(4);

    let candidates = v.get("candidates").and_then(|c| c.as_array());
    let candidate = candidates.and_then(|a| a.first());

    if let Some(candidate) = candidate {
        let parts = candidate
            .pointer("/content/parts")
            .and_then(|p| p.as_array());

        if let Some(parts) = parts {
            let mut tool_index = 0u32;
            for part in parts {
                // 推理过程（thought: true）
                if part
                    .get("thought")
                    .and_then(|t| t.as_bool())
                    .unwrap_or(false)
                {
                    if let Some(text) = part
                        .get("text")
                        .and_then(|t| t.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        chunks.push(StreamChunk::Thinking(text.to_string()));
                        continue;
                    }
                }

                // Function call — ToolCallStart + ToolCallDelta（完整参数）
                if let Some(fc) = part.get("functionCall") {
                    let name = fc
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    let args = fc
                        .get("args")
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| "{}".to_string());
                    chunks.push(StreamChunk::ToolCallStart {
                        index: tool_index,
                        id: format!("call_{name}_{tool_index}"),
                        name,
                        signature: None,
                    });
                    chunks.push(StreamChunk::ToolCallDelta {
                        index: tool_index,
                        arguments: args,
                    });
                    tool_index += 1;
                    continue;
                }

                // 文本内容
                if let Some(text) = part
                    .get("text")
                    .and_then(|t| t.as_str())
                    .filter(|s| !s.is_empty())
                {
                    chunks.push(StreamChunk::Text(text.to_string()));
                }
            }
        }

        // 结束原因
        if let Some(reason) = candidate
            .get("finishReason")
            .and_then(|f| f.as_str())
            .filter(|s| !s.is_empty())
        {
            let mapped = match reason {
                "STOP" => "stop",
                "MAX_TOKENS" => "length",
                "FUNCTION_CALL" => "tool_calls",
                other => other,
            };
            chunks.push(StreamChunk::Done {
                finish_reason: mapped.to_string(),
            });
        }
    }

    // Token 用量（usageMetadata）
    if let Some(u) = v.get("usageMetadata") {
        let input = u
            .get("promptTokenCount")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as u32;
        let output = u
            .get("candidatesTokenCount")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as u32;
        let reasoning = u
            .get("thoughtsTokenCount")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as u32;
        let cached = u
            .get("cachedContentTokenCount")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as u32;
        if input > 0 || output > 0 {
            chunks.push(StreamChunk::Usage(Usage {
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: cached,
                cache_write_tokens: 0,
                reasoning_tokens: reasoning,
                request_count: 1,
            }));
        }
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::client::ChatClient;
    use crate::types::StreamChunk;

    #[test]
    fn gemini_native_has_chat() {
        let client = ProviderClient::new("test-key", GeminiNative);
        let _model = client.completion_model("gemini-3.6-flash");
    }

    #[test]
    fn system_instruction_extracted() {
        let msgs = vec![
            crate::types::Message::system("Be helpful"),
            crate::types::Message::developer("Follow project policy"),
            crate::types::Message::user_text("Hi"),
        ];
        let (sys, contents) = to_native_contents(&msgs);
        assert_eq!(sys.as_deref(), Some("Be helpful\n\nFollow project policy"));
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
    }

    #[test]
    fn extract_text_from_candidates() {
        let data = r#"{"candidates":[{"content":{"parts":[{"text":"Hello"}],"role":"model"}}]}"#;
        let chunks = extract_native_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Text(t) if t == "Hello"));
    }

    #[test]
    fn extract_thinking_from_thought_part() {
        let data = r#"{"candidates":[{"content":{"parts":[{"thought":true,"text":"let me think"}],"role":"model"}}]}"#;
        let chunks = extract_native_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Thinking(t) if t == "let me think"));
    }

    #[test]
    fn extract_function_call_with_args() {
        let data = r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"get_weather","args":{"location":"SF"}}}],"role":"model"}}]}"#;
        let chunks = extract_native_chunks(data);
        assert_eq!(chunks.len(), 2);
        assert!(
            matches!(&chunks[0], StreamChunk::ToolCallStart { ref name, .. } if name == "get_weather")
        );
        assert!(
            matches!(&chunks[1], StreamChunk::ToolCallDelta { ref arguments, .. } if arguments.contains("SF"))
        );
    }

    #[test]
    fn extract_usage_metadata() {
        let data = r#"{"candidates":[{"content":{"parts":[{"text":"done"}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":5}}"#;
        let chunks = extract_native_chunks(data);
        let types: Vec<&str> = chunks
            .iter()
            .map(|c| match c {
                StreamChunk::Text(_) => "Text",
                StreamChunk::Done { .. } => "Done",
                StreamChunk::Usage(_) => "Usage",
                _ => "Other",
            })
            .collect();
        assert_eq!(types, vec!["Text", "Done", "Usage"]);
        if let StreamChunk::Usage(u) = &chunks[2] {
            assert_eq!(u.input_tokens, 10);
            assert_eq!(u.output_tokens, 5);
        }
    }

    #[test]
    fn extract_finish_reason_stop() {
        let data = r#"{"candidates":[{"finishReason":"STOP"}]}"#;
        let chunks = extract_native_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], StreamChunk::Done { ref finish_reason } if finish_reason == "stop")
        );
    }

    #[test]
    fn extract_error() {
        let data = r#"{"error":{"message":"Invalid API key"}}"#;
        let chunks = extract_native_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Error(ref msg) if msg == "Invalid API key"));
    }

    #[test]
    fn extract_thinking_then_function_call() {
        let data = r#"{"candidates":[{"content":{"parts":[{"thought":true,"text":"analyzing"},{"functionCall":{"name":"search","args":{"q":"rust"}}}],"role":"model"},"finishReason":"FUNCTION_CALL"}]}"#;
        let chunks = extract_native_chunks(data);
        let types: Vec<&str> = chunks
            .iter()
            .map(|c| match c {
                StreamChunk::Thinking(_) => "Thinking",
                StreamChunk::ToolCallStart { .. } => "ToolCallStart",
                StreamChunk::ToolCallDelta { .. } => "ToolCallDelta",
                StreamChunk::Done { .. } => "Done",
                _ => "Other",
            })
            .collect();
        assert_eq!(
            types,
            vec!["Thinking", "ToolCallStart", "ToolCallDelta", "Done"]
        );
    }

    #[test]
    fn tool_call_in_assistant_message() {
        let msgs = vec![crate::types::Message::assistant(vec![
            crate::types::AssistantContent::ToolCall(crate::types::message::ToolCall {
                id: "call_1".into(),
                name: "search".into(),
                arguments: json!({"q": "rust"}),
                signature: None,
            }),
        ])];
        let (_, contents) = to_native_contents(&msgs);
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "model");
        assert!(contents[0]["parts"][0].get("functionCall").is_some());
        assert_eq!(contents[0]["parts"][0]["functionCall"]["name"], "search");
    }
}
