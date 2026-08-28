//! SSE 流解析基础设施（所有厂商共用）。

use std::collections::VecDeque;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use futures::StreamExt;

use crate::types::stream::{CompletionStream, StreamChunk};

fn take_complete_line(buf: &mut Vec<u8>) -> Option<String> {
    let newline = buf.iter().position(|byte| *byte == b'\n')?;
    let mut line = buf.drain(..=newline).collect::<Vec<_>>();
    line.pop();
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Some(String::from_utf8_lossy(&line).into_owned())
}

/// SSE 事件提取器：解析 `data:` 负载为零或多个 [`StreamChunk`]。
pub type ChunkExtract = Arc<dyn Fn(&str) -> Vec<StreamChunk> + Send + Sync>;

/// 将旧式 `Option<StreamChunk>` 提取器包装为 `ChunkExtract`。
pub fn wrap_single_extract(
    f: impl Fn(&str) -> Option<StreamChunk> + Send + Sync + 'static,
) -> ChunkExtract {
    Arc::new(move |data: &str| f(data).into_iter().collect())
}

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
        (
            byte_stream,
            Vec::<u8>::new(),
            false,
            extract,
            VecDeque::<StreamChunk>::new(),
        ),
        |(mut byte_stream, mut buf, done, extract, mut pending)| async move {
            // 先排空 pending 队列（一个 SSE 事件可产出多个 chunk）
            if let Some(chunk) = pending.pop_front() {
                return Some((Ok(chunk), (byte_stream, buf, done, extract, pending)));
            }

            if done {
                return None;
            }
            loop {
                if let Some(line) = take_complete_line(&mut buf) {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with(':') {
                        continue;
                    }
                    if let Some(data) = trimmed.strip_prefix("data:") {
                        let data = data.trim();
                        if data == "[DONE]" {
                            return None;
                        }
                        let mut chunks = extract(data);
                        if chunks.is_empty() {
                            continue;
                        }
                        // 第一个 chunk 检查是否为 Error
                        if matches!(&chunks[0], StreamChunk::Error(_)) {
                            let msg = match chunks.remove(0) {
                                StreamChunk::Error(m) => m,
                                _ => unreachable!(),
                            };
                            return Some((
                                Err(anyhow!(msg)),
                                (byte_stream, buf, true, extract, pending),
                            ));
                        }
                        let first = chunks.remove(0);
                        for rest in chunks {
                            pending.push_back(rest);
                        }
                        return Some((Ok(first), (byte_stream, buf, false, extract, pending)));
                    }
                    continue;
                }

                match byte_stream.next().await {
                    Some(Ok(bytes)) => {
                        buf.extend_from_slice(&bytes);
                    }
                    Some(Err(err)) => {
                        return Some((Err(err.into()), (byte_stream, buf, true, extract, pending)));
                    }
                    None => {
                        if buf.iter().any(|byte| !byte.is_ascii_whitespace()) {
                            let line = String::from_utf8_lossy(&buf).trim().to_string();
                            buf.clear();
                            if let Some(data) = line.strip_prefix("data:") {
                                let data = data.trim();
                                if data != "[DONE]" {
                                    let mut chunks = extract(data);
                                    if let Some(first) = chunks.first() {
                                        if matches!(first, StreamChunk::Error(_)) {
                                            let msg = match chunks.remove(0) {
                                                StreamChunk::Error(m) => m,
                                                _ => unreachable!(),
                                            };
                                            return Some((
                                                Err(anyhow!(msg)),
                                                (byte_stream, buf, true, extract, pending),
                                            ));
                                        }
                                        let first = chunks.remove(0);
                                        for rest in chunks {
                                            pending.push_back(rest);
                                        }
                                        return Some((
                                            Ok(first),
                                            (byte_stream, buf, true, extract, pending),
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

#[cfg(test)]
mod tests {
    use super::take_complete_line;

    #[test]
    fn complete_line_preserves_utf8_split_across_chunks() {
        let line = "data: {\"delta\":\"中央\"}\n";
        let chinese_start = line.find('中').expect("fixture contains Chinese text");
        let split = chinese_start + 1;
        let bytes = line.as_bytes();
        let mut buffer = bytes[..split].to_vec();

        assert_eq!(take_complete_line(&mut buffer), None);

        buffer.extend_from_slice(&bytes[split..]);
        assert_eq!(
            take_complete_line(&mut buffer).as_deref(),
            Some("data: {\"delta\":\"中央\"}")
        );
        assert!(buffer.is_empty());
    }
}
