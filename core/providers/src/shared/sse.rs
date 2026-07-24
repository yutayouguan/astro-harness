//! SSE 流解析基础设施（所有厂商共用）。

use std::sync::Arc;

use anyhow::{anyhow, Result};
use futures::StreamExt;

use crate::types::stream::{CompletionStream, StreamChunk};

type ChunkExtract = Arc<dyn Fn(&str) -> Option<StreamChunk> + Send + Sync>;

/// 将 HTTP 响应转为 `CompletionStream`，通过 `extract` 闭包解析 SSE 事件。
pub async fn sse_stream(
    response: reqwest::Response,
    extract: ChunkExtract,
) -> Result<CompletionStream> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.pointer("/error/message")
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
            })
            .unwrap_or(body);
        return Err(anyhow!("HTTP {status}: {msg}"));
    }

    let byte_stream = response.bytes_stream();
    let stream = futures::stream::unfold(
        (byte_stream, String::new(), false, extract),
        |(mut byte_stream, mut buf, done, extract)| async move {
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
                    if let Some(data) = trimmed.strip_prefix("data:") {
                        let data = data.trim();
                        if data == "[DONE]" {
                            return None;
                        }
                        if let Some(chunk) = extract(data) {
                            if matches!(&chunk, StreamChunk::Error(_)) {
                                let msg = match chunk {
                                    StreamChunk::Error(m) => m,
                                    _ => unreachable!(),
                                };
                                return Some((
                                    Err(anyhow!(msg)),
                                    (byte_stream, buf, true, extract),
                                ));
                            }
                            return Some((Ok(chunk), (byte_stream, buf, false, extract)));
                        }
                    }
                    continue;
                }

                match byte_stream.next().await {
                    Some(Ok(bytes)) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Some(Err(err)) => {
                        return Some((Err(err.into()), (byte_stream, buf, true, extract)));
                    }
                    None => {
                        if !buf.trim().is_empty() {
                            let line = buf.trim().to_string();
                            buf.clear();
                            if let Some(data) = line.strip_prefix("data:") {
                                let data = data.trim();
                                if data != "[DONE]" {
                                    if let Some(chunk) = extract(data) {
                                        return Some((
                                            Ok(chunk),
                                            (byte_stream, buf, true, extract),
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
