use anyhow::Result;
use async_trait::async_trait;

use crate::engine::executor::{NodeExecutor, NodeResult};
use crate::engine::variables::VariableContext;
use crate::model::WorkflowNode;

macro_rules! media_stub {
    ($name:ident, $label:expr, $fields:expr) => {
        pub struct $name;

        #[async_trait]
        impl NodeExecutor for $name {
            async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
                let prompt_key = $fields;
                let prompt_tpl = node.config.get(prompt_key).and_then(|v| v.as_str()).unwrap_or("");
                let prompt = ctx.interpolate(prompt_tpl);
                let provider = node.config.get("provider_id").and_then(|v| v.as_str()).unwrap_or("");
                let model = node.config.get("model").and_then(|v| v.as_str()).unwrap_or("");
                Ok(NodeResult::Success(serde_json::json!({
                    "type": $label,
                    "prompt": prompt,
                    "provider_id": provider,
                    "model": model,
                    "note": concat!($label, " 待接入 providers crate")
                })))
            }
        }
    };
}

media_stub!(ImageGenExec, "image_generation", "prompt_template");
media_stub!(VideoGenExec, "video_generation", "prompt_template");
media_stub!(MusicGenExec, "music_generation", "prompt_template");
media_stub!(TtsExec, "text_to_speech", "text_template");
media_stub!(SubtitleGenExec, "subtitle_generation", "audio_source");
