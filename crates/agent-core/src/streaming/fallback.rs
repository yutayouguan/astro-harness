//! 聊天主模型故障切换：错误分类与首包前 fallback 流包装。

use futures::{stream, StreamExt};
use providers::types::message::Message as ProviderMessage;
use providers::types::stream::{CompletionStream, StreamChunk};
use providers::ProviderConfig;
use types::ChatTarget;

/// 实际命中目标的可观测元数据（写入 usage 等，不改会话默认模型）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveTargetMeta {
    pub provider_id: String,
    pub backend_id: String,
    pub model: String,
    pub base_url: String,
}

impl ActiveTargetMeta {
    fn from_target(t: &ChatTarget) -> Self {
        Self {
            provider_id: t.provider_id.clone(),
            backend_id: t.backend_id.clone(),
            model: t.model.clone(),
            base_url: t.base_url.clone(),
        }
    }
}

/// 判断错误是否允许在首包前 failover 到链上下一家。
///
/// 可切：429 / rate limit、401/403、5xx、超时/连接/TLS/DNS 等。
/// 不可切：400 等坏请求、用户取消（除非同时呈限流语义）。
pub fn is_failover_eligible(err: &anyhow::Error) -> bool {
    let s = err.to_string().to_ascii_lowercase();

    let cancel_like = s.contains("cancelled") || s.contains("canceled") || s.contains("aborted");
    let rate_limit_like = s.contains("429") || s.contains("rate limit");
    if cancel_like && !rate_limit_like {
        return false;
    }

    if s.contains("429")
        || s.contains("rate limit")
        || s.contains("401")
        || s.contains("403")
        || s.contains("unauthorized")
        || s.contains("forbidden")
        || s.contains("502")
        || s.contains("503")
        || s.contains("504")
        || s.contains("timeout")
        || s.contains("connection")
        || s.contains("tls")
        || s.contains("dns")
        || has_5xx_status(&s)
    {
        return true;
    }

    false
}

/// 匹配独立的 HTTP 5xx 状态码（避免把 `timeout 5000ms` 误判为 500）。
fn has_5xx_status(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'5' && bytes[i + 1].is_ascii_digit() && bytes[i + 2].is_ascii_digit() {
            let prev_ok = i == 0 || !bytes[i - 1].is_ascii_digit();
            let next_ok = i + 3 >= bytes.len() || !bytes[i + 3].is_ascii_digit();
            if prev_ok && next_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn chunk_has_meaningful_content(chunk: &StreamChunk) -> bool {
    match chunk {
        StreamChunk::Text(t) if !t.is_empty() => true,
        StreamChunk::Thinking(t) if !t.is_empty() => true,
        StreamChunk::ToolCallStart { .. } | StreamChunk::ToolCallDelta { .. } => true,
        _ => false,
    }
}

fn is_error_only_pre_content_chunk(chunk: &StreamChunk) -> bool {
    matches!(chunk, StreamChunk::Error(_)) && !chunk_has_meaningful_content(chunk)
}

/// Peek 首个流事件：首包前错误（流 Err 或 error-only chunk）直接失败；否则还原含已 peek 项的流。
pub async fn probe_or_wrap_pre_content(
    mut stream: CompletionStream,
) -> anyhow::Result<CompletionStream> {
    match stream.next().await {
        None => Ok(stream),
        Some(Err(e)) => Err(e),
        Some(Ok(chunk)) => {
            if is_error_only_pre_content_chunk(&chunk) {
                if let StreamChunk::Error(msg) = chunk {
                    return Err(anyhow::anyhow!("error: {msg}"));
                }
                unreachable!()
            }
            Ok(Box::pin(
                stream::once(async move { Ok(chunk) }).chain(stream),
            ))
        }
    }
}

/// 按 `targets` 链尝试 `chat_stream`；仅首包前可切；耗尽返回聚合错误。
pub async fn try_stream_completion_with_fallback(
    targets: &[ChatTarget],
    messages: Vec<ProviderMessage>,
    tools: Vec<serde_json::Value>,
    base_config: &ProviderConfig,
    mut on_failover: impl FnMut(&ChatTarget, &ChatTarget, &anyhow::Error),
) -> anyhow::Result<(CompletionStream, ActiveTargetMeta)> {
    if targets.is_empty() {
        anyhow::bail!("聊天目标列表为空，无法发起补全");
    }

    let mut errors = Vec::new();
    for (i, target) in targets.iter().enumerate() {
        let is_google = target.backend_id == "google" || target.provider_id == "google";
        let config = ProviderConfig {
            api_key: target.api_key.clone(),
            base_url: Some(target.base_url.clone()).filter(|s| !s.is_empty()),
            model: target.model.clone(),
            temperature: base_config.temperature,
            max_tokens: base_config.max_tokens,
            thinking_enabled: base_config.thinking_enabled,
            reasoning_effort: base_config.reasoning_effort.clone(),
            additional_params: base_config.additional_params.clone(),
            // 仅 Google Interactions 续写；其它后端忽略该字段
            previous_interaction_id: if is_google {
                base_config.previous_interaction_id.clone()
            } else {
                None
            },
            api_mode: target.api_mode.clone(),
        };

        let attempt = async {
            let stream = providers::dispatch::chat_stream(
                &target.backend_id,
                messages.clone(),
                tools.clone(),
                &config,
            )
            .await?;
            probe_or_wrap_pre_content(stream).await
        }
        .await;

        match attempt {
            Ok(stream) => {
                return Ok((stream, ActiveTargetMeta::from_target(target)));
            }
            Err(e) if is_failover_eligible(&e) && i + 1 < targets.len() => {
                on_failover(target, &targets[i + 1], &e);
                errors.push(format!("{}: {e:#}", target.backend_id));
            }
            Err(e) => {
                errors.push(format!("{}: {e:#}", target.backend_id));
                break;
            }
        }
    }

    anyhow::bail!("全部模型尝试失败：{}", errors.join("；"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failover_eligible_for_429_and_5xx() {
        assert!(is_failover_eligible(&anyhow::anyhow!(
            "HTTP 429 Too Many Requests"
        )));
        assert!(is_failover_eligible(&anyhow::anyhow!(
            "status 503 service unavailable"
        )));
        assert!(is_failover_eligible(&anyhow::anyhow!("401 Unauthorized")));
        assert!(is_failover_eligible(&anyhow::anyhow!(
            "connection reset by peer"
        )));
        assert!(!is_failover_eligible(&anyhow::anyhow!(
            "HTTP 400 bad request"
        )));
        assert!(!is_failover_eligible(&anyhow::anyhow!("cancelled by user")));
    }

    #[test]
    fn failover_eligible_edge_cases() {
        assert!(is_failover_eligible(&anyhow::anyhow!("403 Forbidden")));
        assert!(is_failover_eligible(&anyhow::anyhow!(
            "rate limit exceeded"
        )));
        assert!(is_failover_eligible(&anyhow::anyhow!(
            "tls handshake failure"
        )));
        assert!(is_failover_eligible(&anyhow::anyhow!(
            "dns resolution failed"
        )));
        assert!(is_failover_eligible(&anyhow::anyhow!("request timeout")));
        assert!(is_failover_eligible(&anyhow::anyhow!(
            "HTTP 500 Internal Server Error"
        )));
        assert!(!is_failover_eligible(&anyhow::anyhow!("aborted by client")));
        assert!(!is_failover_eligible(&anyhow::anyhow!("canceled")));
        // 同时含取消与限流语义 → 仍可切
        assert!(is_failover_eligible(&anyhow::anyhow!(
            "cancelled: HTTP 429"
        )));
        // 勿把 ms 超时数字当成 5xx
        assert!(!is_failover_eligible(&anyhow::anyhow!(
            "waited 5000ms then gave up"
        )));
    }

    #[tokio::test]
    async fn probe_treats_error_only_first_chunk_as_failure() {
        let stream: CompletionStream = Box::pin(stream::iter(vec![Ok(StreamChunk::Error(
            "rate limit".into(),
        ))]));
        let err = match probe_or_wrap_pre_content(stream).await {
            Ok(_) => panic!("error-only chunk should fail"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("error: rate limit"));
        assert!(is_failover_eligible(&err));
    }

    #[tokio::test]
    async fn probe_preserves_meaningful_first_chunk() {
        let stream: CompletionStream = Box::pin(stream::iter(vec![
            Ok(StreamChunk::Text("hi".into())),
            Ok(StreamChunk::Text("!".into())),
        ]));
        let mut wrapped = probe_or_wrap_pre_content(stream).await.expect("ok");
        let first = wrapped.next().await.unwrap().unwrap();
        assert!(matches!(first, StreamChunk::Text(ref t) if t == "hi"));
        let second = wrapped.next().await.unwrap().unwrap();
        assert!(matches!(second, StreamChunk::Text(ref t) if t == "!"));
    }

    #[tokio::test]
    async fn probe_keeps_error_chunk_when_content_already_present() {
        // 在新模型中，单个 chunk 只能是一种变体。
        // 模拟：一个文本 chunk 后跟一个错误 chunk。
        let stream: CompletionStream = Box::pin(stream::iter(vec![
            Ok(StreamChunk::Text("partial".into())),
            Ok(StreamChunk::Error("mid-stream".into())),
        ]));
        let mut wrapped = probe_or_wrap_pre_content(stream).await.expect("locked");
        let chunk = wrapped.next().await.unwrap().unwrap();
        assert!(matches!(chunk, StreamChunk::Text(ref t) if t == "partial"));
    }

    #[tokio::test]
    async fn try_stream_rejects_empty_targets() {
        let err = match try_stream_completion_with_fallback(
            &[],
            vec![],
            vec![],
            &ProviderConfig::default(),
            |_, _, _| {},
        )
        .await
        {
            Ok(_) => panic!("empty targets should fail"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("空"));
    }
}
