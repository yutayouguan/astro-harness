//! [`ProviderStreamer`]：包装 [`ProviderRegistry`] + fallback 链，实现三层 Streaming trait。

use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use common::message::Message;
use common::ChatTarget;
use futures::StreamExt;
use providers::registry::ProviderRegistry;
use providers::trait_::{AiProvider, ChatMessage as ProviderMessage, ChatStream, ProviderConfig};

use crate::prompt::messages::to_provider_messages;

use super::fallback::{try_stream_completion_with_fallback, ActiveTargetMeta};
use super::traits::{StreamingChat, StreamingCompletion, StreamingPrompt};
use super::types::{map_provider_stream, AssistantContentStream};

/// 包装 [`ProviderRegistry`] + fallback 链，实现三层 Streaming trait。
pub struct ProviderStreamer {
    /// 按 `backend_id` 解析具体 [`AiProvider`]。
    pub registry: Arc<ProviderRegistry>,
    /// 含 primary 的聊天目标链（失败切模仅用此列表，不改会话默认凭据）。
    pub targets: Vec<ChatTarget>,
    /// temperature / thinking 等；model/key/url 由每跳 target 覆盖。
    pub base_config: ProviderConfig,
    /// 最近一次成功补全命中的目标元数据（供 usage 记录）。
    last_hit: StdMutex<Option<ActiveTargetMeta>>,
    /// Google Interactions：上一轮 `interaction.id`，供工具多轮 `previous_interaction_id`。
    previous_interaction_id: Arc<StdMutex<Option<String>>>,
}

impl ProviderStreamer {
    pub fn new(
        registry: Arc<ProviderRegistry>,
        targets: Vec<ChatTarget>,
        base_config: ProviderConfig,
    ) -> Self {
        Self {
            registry,
            targets,
            base_config,
            last_hit: StdMutex::new(None),
            previous_interaction_id: Arc::new(StdMutex::new(None)),
        }
    }

    /// 最近一次成功 stream 的命中元数据。
    pub fn last_hit_meta(&self) -> Option<ActiveTargetMeta> {
        self.last_hit.lock().ok().and_then(|g| g.clone())
    }

    pub(crate) fn api_key_for(&self, meta: &ActiveTargetMeta) -> String {
        self.targets
            .iter()
            .find(|t| {
                t.backend_id == meta.backend_id
                    && t.model == meta.model
                    && (meta.provider_id.is_empty() || t.provider_id == meta.provider_id)
            })
            .map(|t| t.api_key.clone())
            .unwrap_or_else(|| self.base_config.api_key.clone())
    }

    pub(crate) fn primary_model(&self) -> String {
        self.targets
            .first()
            .map(|t| t.model.clone())
            .unwrap_or_else(|| self.base_config.model.clone())
    }
}

/// 从单 Provider + config 构造单元素 fallback 链（旧调用方兼容）。
pub fn chat_target_from_provider_config(
    provider: &dyn AiProvider,
    config: &ProviderConfig,
) -> ChatTarget {
    let backend_id = provider.name().to_string();
    ChatTarget {
        provider_id: backend_id.clone(),
        backend_id,
        model: config.model.clone(),
        api_key: config.api_key.clone(),
        base_url: config.base_url.clone().unwrap_or_default(),
    }
}

/// 将自定义/测试 Provider 注入注册表，并返回单元素 `targets`。
pub fn targets_and_registry_from_primary(
    provider: Arc<dyn AiProvider>,
    config: &ProviderConfig,
) -> (Vec<ChatTarget>, Arc<ProviderRegistry>) {
    let target = chat_target_from_provider_config(provider.as_ref(), config);
    let mut registry = ProviderRegistry::new();
    registry.insert(target.backend_id.clone(), provider);
    (vec![target], Arc::new(registry))
}

#[async_trait]
impl StreamingCompletion for ProviderStreamer {
    /// 经 [`try_stream_completion_with_fallback`] 再 [`map_provider_stream`] 归一化。
    ///
    /// Google Interactions：自动注入/更新 `previous_interaction_id`，使工具多轮
    /// 保留服务端 thought/signature。
    async fn stream_completion(
        &self,
        messages: Vec<ProviderMessage>,
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream> {
        let mut config = self.base_config.clone();
        config.previous_interaction_id = self
            .previous_interaction_id
            .lock()
            .ok()
            .and_then(|g| g.clone());

        let (stream, meta) = try_stream_completion_with_fallback(
            &self.targets,
            self.registry.as_ref(),
            messages,
            tools,
            &config,
            |from, to, err| {
                tracing::warn!(
                    from_backend = %from.backend_id,
                    from_model = %from.model,
                    to_backend = %to.backend_id,
                    to_model = %to.model,
                    error = %err,
                    "chat failover: switching target before first content"
                );
            },
        )
        .await?;

        let is_google = meta.backend_id == "google" || meta.provider_id == "google";
        if !is_google {
            if let Ok(mut guard) = self.previous_interaction_id.lock() {
                *guard = None;
            }
        }

        let prev_slot = if is_google {
            Some(Arc::clone(&self.previous_interaction_id))
        } else {
            None
        };
        let tracked: ChatStream = Box::pin(futures::stream::unfold(
            (stream, prev_slot),
            |(mut stream, prev_slot)| async move {
                match stream.next().await {
                    Some(item) => {
                        if let (Some(ref slot), Ok(ref chunk)) = (&prev_slot, &item) {
                            if let Some(ref id) = chunk.interaction_id {
                                if let Ok(mut g) = slot.lock() {
                                    *g = Some(id.clone());
                                }
                            }
                        }
                        Some((item, (stream, prev_slot)))
                    }
                    None => None,
                }
            },
        ));

        if let Ok(mut guard) = self.last_hit.lock() {
            *guard = Some(meta);
        }
        Ok(map_provider_stream(tracked))
    }
}

#[async_trait]
impl StreamingChat for ProviderStreamer {
    /// 通过 [`to_provider_messages`] 转换历史后调用 `stream_completion`。
    async fn stream_chat(
        &self,
        system_prompt: &str,
        history: &[Message],
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream> {
        let messages = to_provider_messages(system_prompt, history);
        self.stream_completion(messages, tools).await
    }
}

#[async_trait]
impl StreamingPrompt for ProviderStreamer {
    /// 构造单条 user 历史后委托 `stream_chat`。
    async fn stream_prompt(
        &self,
        system_prompt: &str,
        prompt: &str,
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream> {
        let history = vec![Message::user(prompt)];
        self.stream_chat(system_prompt, &history, tools).await
    }
}
