//! LLM 模型配置、凭证与 fallback 链。
//!
//! 从 `AgentLoop` 提取的数据分组，减少主结构体的认知负荷。
//! 所有仅涉及模型/凭证/目标链的方法均在本 struct 上实现，
//! `AgentLoop` 通过薄代理转发保持 API 兼容。

use std::collections::HashMap;

use common::{AuxiliaryTask, ChatTarget, ModelSpec, MAX_CHAT_FALLBACKS};
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
    // ── 主目标 ─────────────────────────────────────────────

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

    // ── 凭证 ───────────────────────────────────────────────

    pub fn set_credentials(
        &mut self,
        provider: &str,
        model: &str,
        api_key: &str,
        base_url: &str,
    ) {
        self.chat_provider = provider.to_string();
        self.chat_model = model.to_string();
        self.chat_api_key = api_key.to_string();
        self.chat_base_url = base_url.to_string();
        if !provider.trim().is_empty() || !model.trim().is_empty() {
            self.model_spec = Some(ModelSpec::new(provider, model));
        }
    }

    pub fn chat_api_key(&self) -> &str {
        &self.chat_api_key
    }

    pub fn chat_base_url(&self) -> &str {
        &self.chat_base_url
    }

    pub fn chat_provider(&self) -> &str {
        &self.chat_provider
    }

    pub fn chat_model(&self) -> &str {
        &self.chat_model
    }

    // ── 聊天目标链 ─────────────────────────────────────────

    /// 设置含 primary 的聊天 fallback 链。
    pub fn set_chat_targets(&mut self, targets: Vec<ChatTarget>) {
        if let Some(primary) = targets.first() {
            self.chat_provider = primary.backend_id.clone();
            self.chat_model = primary.model.clone();
            self.chat_api_key = primary.api_key.clone();
            self.chat_base_url = primary.base_url.clone();
            self.model_spec = Some(ModelSpec::new(&primary.backend_id, &primary.model));
        }
        self.chat_targets = targets;
    }

    pub fn chat_targets(&self) -> &[ChatTarget] {
        &self.chat_targets
    }

    /// 设置 fallback 链（保留 primary，追加去重后的备用目标）。
    pub fn set_fallback_models(&mut self, specs: &[ModelSpec]) {
        let primary = self.primary_chat_target();
        let mut chain = vec![primary.clone()];
        let mut seen = std::collections::HashSet::new();
        seen.insert(primary.provider_id.clone());
        for spec in specs.iter().take(MAX_CHAT_FALLBACKS * 2) {
            if chain.len() > MAX_CHAT_FALLBACKS {
                break;
            }
            let t = spec.apply_to(&primary);
            if t.provider_id.trim().is_empty() || !seen.insert(t.provider_id.clone()) {
                continue;
            }
            chain.push(t);
        }
        self.chat_targets = chain;
    }

    // ── 辅助任务目标 ───────────────────────────────────────

    pub fn set_auxiliary_targets(
        &mut self,
        targets: HashMap<AuxiliaryTask, Vec<ChatTarget>>,
    ) {
        self.auxiliary_targets = targets;
    }

    /// 返回指定辅助任务的目标链；未配置时回退到主目标。
    pub fn auxiliary_targets(&self, task: AuxiliaryTask) -> Vec<ChatTarget> {
        if let Some(targets) = self.auxiliary_targets.get(&task) {
            if !targets.is_empty() {
                return targets.clone();
            }
        }
        match self.chat_targets.first() {
            Some(primary) => vec![primary.clone()],
            None => vec![ChatTarget {
                provider_id: String::new(),
                backend_id: self.chat_provider.clone(),
                model: self.chat_model.clone(),
                api_key: self.chat_api_key.clone(),
                base_url: self.chat_base_url.clone(),
            }],
        }
    }

    // ── 模型声明 ───────────────────────────────────────────

    pub fn model_spec(&self) -> Option<&ModelSpec> {
        self.model_spec.as_ref()
    }

    // ── 上下文窗口 ─────────────────────────────────────────

    pub fn set_context_window(&mut self, window: u32) {
        self.context_window = if window == 0 {
            crate::prompt::context_usage::DEFAULT_CONTEXT_WINDOW
        } else {
            window
        };
    }

    pub fn context_window(&self) -> u32 {
        if self.context_window == 0 {
            crate::prompt::context_usage::DEFAULT_CONTEXT_WINDOW
        } else {
            self.context_window
        }
    }

    // ── 图像生成 ───────────────────────────────────────────

    pub fn set_image_gen_targets(&mut self, targets: ImageGenTargets) {
        self.image_gen_targets = targets;
    }

    pub fn image_gen_targets(&self) -> &ImageGenTargets {
        &self.image_gen_targets
    }
}
