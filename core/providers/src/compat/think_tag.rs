//! 从流式文本中提取 `<think>…</think>` 标签，转为 `StreamChunk::Thinking`。
//!
//! 用于没有原生 `reasoning_content` 字段但在文本中输出 `<think>` 标签的 provider。
//! 已有 `reasoning_content` 的 provider（如 DeepSeek）不会在文本中出现该标签，包装器为无操作。

use std::collections::VecDeque;

use futures::StreamExt;

use crate::types::stream::{CompletionStream, StreamChunk};

const OPEN_TAG: &str = "<think>";
const CLOSE_TAG: &str = "</think>";

/// 包装 `CompletionStream`，将文本中的 `<think>…</think>` 提取为 `Thinking` chunk。
pub fn wrap_think_tag_extraction(stream: CompletionStream) -> CompletionStream {
    Box::pin(futures::stream::unfold(
        (
            stream,
            ThinkTagExtractor::new(),
            VecDeque::<Result<StreamChunk, anyhow::Error>>::new(),
            false,
        ),
        |(mut stream, mut ext, mut pending, mut done)| async move {
            if let Some(item) = pending.pop_front() {
                return Some((item, (stream, ext, pending, done)));
            }

            if done {
                return None;
            }

            loop {
                match stream.next().await {
                    Some(Ok(chunk)) => {
                        let chunks = ext.process_chunk(chunk);
                        if chunks.is_empty() {
                            continue;
                        }
                        let mut iter = chunks.into_iter();
                        let first = iter.next().unwrap();
                        for rest in iter {
                            pending.push_back(Ok(rest));
                        }
                        return Some((Ok(first), (stream, ext, pending, done)));
                    }
                    Some(Err(e)) => {
                        return Some((Err(e), (stream, ext, pending, done)));
                    }
                    None => {
                        let remaining = ext.flush();
                        done = true;
                        if remaining.is_empty() {
                            return None;
                        }
                        let mut iter = remaining.into_iter();
                        let first = iter.next().unwrap();
                        for rest in iter {
                            pending.push_back(Ok(rest));
                        }
                        return Some((Ok(first), (stream, ext, pending, done)));
                    }
                }
            }
        },
    ))
}

struct ThinkTagExtractor {
    in_think: bool,
    buf: String,
}

impl ThinkTagExtractor {
    fn new() -> Self {
        Self {
            in_think: false,
            buf: String::new(),
        }
    }

    fn process_chunk(&mut self, chunk: StreamChunk) -> Vec<StreamChunk> {
        match chunk {
            StreamChunk::Text(text) => self.process_text(&text),
            other => vec![other],
        }
    }

    fn process_text(&mut self, text: &str) -> Vec<StreamChunk> {
        self.buf.push_str(text);
        let mut results = Vec::new();

        loop {
            if !self.in_think {
                if let Some(idx) = self.buf.find(OPEN_TAG) {
                    let before = self.buf[..idx].to_string();
                    if !before.is_empty() {
                        results.push(StreamChunk::Text(before));
                    }
                    self.in_think = true;
                    self.buf = self.buf[idx + OPEN_TAG.len()..].to_string();
                } else {
                    let (emit, keep) = split_at_partial_prefix(&self.buf, OPEN_TAG);
                    if !emit.is_empty() {
                        results.push(StreamChunk::Text(emit.to_string()));
                    }
                    self.buf = keep.to_string();
                    break;
                }
            } else if let Some(idx) = self.buf.find(CLOSE_TAG) {
                let before = self.buf[..idx].to_string();
                if !before.is_empty() {
                    results.push(StreamChunk::Thinking(before));
                }
                self.in_think = false;
                self.buf = self.buf[idx + CLOSE_TAG.len()..].to_string();
            } else {
                let (emit, keep) = split_at_partial_prefix(&self.buf, CLOSE_TAG);
                if !emit.is_empty() {
                    results.push(StreamChunk::Thinking(emit.to_string()));
                }
                self.buf = keep.to_string();
                break;
            }
        }

        results
    }

    fn flush(&mut self) -> Vec<StreamChunk> {
        if self.buf.is_empty() {
            return Vec::new();
        }
        let text = std::mem::take(&mut self.buf);
        if self.in_think {
            vec![StreamChunk::Thinking(text)]
        } else {
            vec![StreamChunk::Text(text)]
        }
    }
}

/// 检查 `text` 尾部是否为 `tag` 的前缀（跨 chunk 标签拆分兜底）。
fn split_at_partial_prefix<'a>(text: &'a str, tag: &str) -> (&'a str, &'a str) {
    let max_check = (tag.len() - 1).min(text.len());
    for suffix_len in (1..=max_check).rev() {
        let split = text.len() - suffix_len;
        if !text.is_char_boundary(split) {
            continue;
        }
        let tail = &text[split..];
        if tag.starts_with(tail) {
            return (&text[..split], tail);
        }
    }
    (text, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_think_tags_simple() {
        let mut ext = ThinkTagExtractor::new();
        let chunks = ext.process_text("<think>hello</think>world");
        assert_eq!(chunks.len(), 2);
        assert!(matches!(&chunks[0], StreamChunk::Thinking(t) if t == "hello"));
        assert!(matches!(&chunks[1], StreamChunk::Text(t) if t == "world"));
    }

    #[test]
    fn no_think_tags() {
        let mut ext = ThinkTagExtractor::new();
        let chunks = ext.process_text("just normal text");
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Text(t) if t == "just normal text"));
    }

    #[test]
    fn think_tag_split_across_chunks() {
        let mut ext = ThinkTagExtractor::new();

        let c1 = ext.process_text("before<thi");
        assert_eq!(c1.len(), 1);
        assert!(matches!(&c1[0], StreamChunk::Text(t) if t == "before"));

        let c2 = ext.process_text("nk>inside</think>after");
        assert_eq!(c2.len(), 2);
        assert!(matches!(&c2[0], StreamChunk::Thinking(t) if t == "inside"));
        assert!(matches!(&c2[1], StreamChunk::Text(t) if t == "after"));
    }

    #[test]
    fn close_tag_split_across_chunks() {
        let mut ext = ThinkTagExtractor::new();

        let c1 = ext.process_text("<think>thinking content</th");
        assert_eq!(c1.len(), 1);
        assert!(matches!(&c1[0], StreamChunk::Thinking(t) if t == "thinking content"));

        let c2 = ext.process_text("ink>normal");
        assert_eq!(c2.len(), 1);
        assert!(matches!(&c2[0], StreamChunk::Text(t) if t == "normal"));
    }

    #[test]
    fn think_only_no_close() {
        let mut ext = ThinkTagExtractor::new();
        let c1 = ext.process_text("<think>unclosed thinking");
        assert_eq!(c1.len(), 1);
        assert!(matches!(&c1[0], StreamChunk::Thinking(t) if t == "unclosed thinking"));

        let flushed = ext.flush();
        assert!(flushed.is_empty());
    }

    #[test]
    fn text_before_think() {
        let mut ext = ThinkTagExtractor::new();
        let chunks = ext.process_text("prefix<think>thought</think>");
        assert_eq!(chunks.len(), 2);
        assert!(matches!(&chunks[0], StreamChunk::Text(t) if t == "prefix"));
        assert!(matches!(&chunks[1], StreamChunk::Thinking(t) if t == "thought"));
    }

    #[test]
    fn flush_emits_buffered_content() {
        let mut ext = ThinkTagExtractor::new();
        let c1 = ext.process_text("<think>partial");
        assert_eq!(c1.len(), 1);

        let flushed = ext.flush();
        assert!(flushed.is_empty()); // "partial" was already emitted
    }

    #[test]
    fn non_text_chunks_pass_through() {
        let mut ext = ThinkTagExtractor::new();
        let chunks = ext.process_chunk(StreamChunk::Done {
            finish_reason: "stop".into(),
        });
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Done { .. }));
    }

    #[test]
    fn partial_prefix_detection() {
        assert_eq!(split_at_partial_prefix("abc<", "<think>"), ("abc", "<"));
        assert_eq!(
            split_at_partial_prefix("abc<thi", "<think>"),
            ("abc", "<thi")
        );
        assert_eq!(
            split_at_partial_prefix("abc<think", "<think>"),
            ("abc", "<think")
        );
        assert_eq!(split_at_partial_prefix("abc", "<think>"), ("abc", ""));
        assert_eq!(
            split_at_partial_prefix("abc<other", "<think>"),
            ("abc<other", "")
        );
    }

    #[test]
    fn partial_prefix_multibyte_no_panic() {
        // 中文字符是 3 字节 UTF-8，按字节切会 panic（旧 bug）
        assert_eq!(split_at_partial_prefix("我", "<think>"), ("我", ""));
        assert_eq!(split_at_partial_prefix("你好", "<think>"), ("你好", ""));
        assert_eq!(split_at_partial_prefix("说<", "<think>"), ("说", "<"));
        assert_eq!(
            split_at_partial_prefix("我<thi", "<think>"),
            ("我", "<thi")
        );
        // emoji (4 字节)
        assert_eq!(split_at_partial_prefix("😊", "<think>"), ("😊", ""));
    }
}
