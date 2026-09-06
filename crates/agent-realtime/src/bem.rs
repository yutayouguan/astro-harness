use std::collections::BTreeMap;
use std::sync::Arc;

use agent_protocol::BemItemPresentation;

const INLINE_MARKDOWN_DIRECTIVE: &str = "::codex-realtime-inline{}";
const INLINE_VISUALIZATION_DIRECTIVE: &str = "::codex-inline-vis{";
const VISUALIZE_DIRECTIVE: &str = "\u{e200}visualize\u{e202}{";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BemPhase {
    Commentary,
    Final,
}

pub fn bem_phase(text: &str, channel_prefixes: &BTreeMap<String, Vec<String>>) -> Option<BemPhase> {
    for (channel, default_prefix, phase) in [
        ("analysis", "[ANALYSIS]", BemPhase::Commentary),
        ("commentary", "[COMMENTARY]", BemPhase::Commentary),
        ("final", "[FINAL]", BemPhase::Final),
    ] {
        let matches = channel_prefixes.get(channel).map_or_else(
            || text.starts_with(default_prefix),
            |prefixes| {
                prefixes
                    .iter()
                    .any(|prefix| !prefix.is_empty() && text.starts_with(prefix))
            },
        );
        if matches {
            return Some(phase);
        }
    }
    None
}

pub fn bem_presentations(text: &str) -> Vec<BemItemPresentation> {
    let mut lines = text.trim_start().lines();
    let mut first = lines.next().unwrap_or_default();
    if first.starts_with('[') {
        if let Some((_, content)) = first.split_once(']') {
            first = content.trim_start();
            if first.is_empty() {
                first = lines.next().unwrap_or_default();
            }
        }
    }
    if first == INLINE_MARKDOWN_DIRECTIVE && text.contains('\n') {
        return vec![BemItemPresentation::InlineMarkdown];
    }
    let mut in_fence = false;
    let mut presentations = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence
            && (trimmed.starts_with(INLINE_VISUALIZATION_DIRECTIVE)
                || trimmed.starts_with(VISUALIZE_DIRECTIVE))
        {
            presentations.push(BemItemPresentation::InlineVisualization {
                index: presentations.len() as u32,
            });
        }
    }
    if presentations.is_empty() {
        presentations.push(BemItemPresentation::WholeItem);
    }
    presentations
}

/// Holds streamed output until a complete BEM channel prefix is recognizable.
#[derive(Debug, Default)]
pub struct BemChannelParser {
    channel_prefixes: Arc<BTreeMap<String, Vec<String>>>,
    buffered_text: String,
    phase: Option<BemPhase>,
}

impl BemChannelParser {
    pub fn new(channel_prefixes: Arc<BTreeMap<String, Vec<String>>>) -> Self {
        Self {
            channel_prefixes,
            ..Self::default()
        }
    }

    pub fn push(&mut self, text: &str) -> Option<String> {
        if self.phase.is_some() {
            return Some(text.to_string());
        }
        self.buffered_text.push_str(text);
        self.phase = bem_phase(&self.buffered_text, &self.channel_prefixes);
        self.phase?;
        Some(std::mem::take(&mut self.buffered_text))
    }

    pub fn phase(&self) -> Option<BemPhase> {
        self.phase
    }

    pub fn finish(&mut self) -> String {
        std::mem::take(&mut self.buffered_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffers_until_channel_is_known() {
        let mut parser = BemChannelParser::default();
        assert_eq!(parser.push("[FIN"), None);
        assert_eq!(parser.push("AL] hello"), Some("[FINAL] hello".into()));
        assert_eq!(parser.phase(), Some(BemPhase::Final));
        assert_eq!(parser.push(" world"), Some(" world".into()));
    }

    #[test]
    fn configured_prefixes_replace_defaults_for_channel() {
        let prefixes = BTreeMap::from([("final".into(), vec!["<final>".into()])]);
        assert_eq!(bem_phase("<final>ok", &prefixes), Some(BemPhase::Final));
        assert_eq!(bem_phase("[FINAL] no", &prefixes), None);
    }

    #[test]
    fn ignores_visualization_directives_inside_code_fences() {
        assert_eq!(
            bem_presentations("[FINAL]\n```text\n::codex-inline-vis{x}\n```"),
            vec![BemItemPresentation::WholeItem]
        );
    }
}
