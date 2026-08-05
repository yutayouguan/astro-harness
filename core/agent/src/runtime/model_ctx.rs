//! LLM 模型配置、凭证与 fallback 链。
//!
//! 从 `AgentLoop` 提取的数据分组，减少主结构体的认知负荷。

use std::collections::HashMap;

use common::{AuxiliaryTask, ChatTarget, ModelSpec};
use tools::ImageGenTargets;

/// LLM 模型配置、凭证与 fallback 链。
pub struct ModelContext {
    pub(crate) chat_api_key: String,
    pub(crate) chat_base_url: String,
    pub(crate) chat_provider: String,
    pub(crate) chat_model: String,
    pub(crate) context_window: u32,
    pub(crate) chat_targets: Vec<ChatTarget>,
    pub(crate) model_spec: Option<ModelSpec>,
    pub(crate) auxiliary_targets: HashMap<AuxiliaryTask, Vec<ChatTarget>>,
    pub(crate) image_gen_targets: ImageGenTargets,
}

impl Default for ModelContext {
    fn default() -> Self {
        Self {
            chat_api_key: String::new(),
            chat_base_url: String::new(),
            chat_provider: String::new(),
            chat_model: String::new(),
            context_window: crate::prompt::context_usage::DEFAULT_CONTEXT_WINDOW,
            chat_targets: Vec::new(),
            model_spec: None,
            auxiliary_targets: HashMap::new(),
            image_gen_targets: ImageGenTargets::default(),
        }
    }
}

impl ModelContext {
    pub fn primary_chat_target(&self) -> ChatTarget {
        self.chat_targets
            .first()
            .cloned()
            .unwrap_or_else(|| ChatTarget {
                provider_id: self.chat_provider.clone(),
                backend_id: self.chat_provider.clone(),
                model: self.chat_model.clone(),
                api_key: self.chat_api_key.clone(),
                base_url: self.chat_base_url.clone(),
            })
    }
}
