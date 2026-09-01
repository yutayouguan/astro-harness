//! [`ProviderStreamer`]：包装 fallback 链，实现三层 Streaming trait。

use std::ops::Deref;
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use futures::StreamExt;
use providers::types::stream::CompletionStream;
use providers::ProviderConfig;
use types::ChatTarget;

use super::fallback::{try_stream_responses_with_fallback, ActiveTargetMeta};
use super::traits::StreamingResponses;
use super::types::{map_new_provider_stream, AssistantContentStream};

/// 测试用 Responses 调用快照，保留顶层 instructions 与原生 item 历史的边界。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResponsesOverrideInput {
    pub instructions: String,
    pub items: Vec<agent_protocol::ResponseItem>,
}

impl Deref for ResponsesOverrideInput {
    type Target = [agent_protocol::ResponseItem];

    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

impl ResponsesOverrideInput {
    pub fn contains_text(&self, expected: &str) -> bool {
        self.items.iter().any(|item| match item {
            agent_protocol::ResponseItem::Message { content, .. } => content.iter().any(|part| {
                matches!(
                    part,
                    agent_protocol::ContentItem::InputText { text }
                        | agent_protocol::ContentItem::OutputText { text }
                        if text == expected
                )
            }),
            _ => false,
        })
    }

    pub fn message_roles(&self) -> impl Iterator<Item = &str> {
        self.items.iter().filter_map(|item| match item {
            agent_protocol::ResponseItem::Message { role, .. } => Some(role.as_str()),
            _ => None,
        })
    }

    pub fn message_summaries(&self) -> Vec<(String, String)> {
        self.items
            .iter()
            .filter_map(|item| match item {
                agent_protocol::ResponseItem::Message { role, content, .. } => Some((
                    role.clone(),
                    content
                        .iter()
                        .filter_map(|part| match part {
                            agent_protocol::ContentItem::InputText { text }
                            | agent_protocol::ContentItem::OutputText { text } => {
                                Some(text.as_str())
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                )),
                _ => None,
            })
            .collect()
    }
}

/// 测试用 Responses 函数覆盖：跳过 dispatch，直接返回脚本化的 CompletionStream。
pub type ResponsesOverride = Arc<
    dyn Fn(
            ResponsesOverrideInput,
            Vec<serde_json::Value>,
            ProviderConfig,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<CompletionStream>> + Send>,
        > + Send
        + Sync,
>;

/// 包装 fallback 链，实现三层 Streaming trait。
pub struct ProviderStreamer {
    /// 含 primary 的聊天目标链（失败切模仅用此列表，不改会话默认凭据）。
    pub targets: Vec<ChatTarget>,
    /// temperature / thinking 等；model/key/url 由每跳 target 覆盖。
    pub base_config: ProviderConfig,
    /// 最近一次成功补全命中的目标元数据（供 usage 记录）。
    last_hit: StdMutex<Option<ActiveTargetMeta>>,
    /// 最近一次已尝试的目标，包括最终失败的 fallback 目标。
    last_attempt: StdMutex<Option<ActiveTargetMeta>>,
    /// 测试覆盖：非空时跳过 dispatch，直接使用此函数获取 CompletionStream。
    responses_override: Option<ResponsesOverride>,
}

impl ProviderStreamer {
    pub fn new(targets: Vec<ChatTarget>, base_config: ProviderConfig) -> Self {
        Self {
            targets,
            base_config,
            last_hit: StdMutex::new(None),
            last_attempt: StdMutex::new(None),
            responses_override: None,
        }
    }

    /// 构造带测试覆盖的 ProviderStreamer（供集成测试注入脚本化回复）。
    pub fn with_responses_override(
        targets: Vec<ChatTarget>,
        base_config: ProviderConfig,
        responses_override: ResponsesOverride,
    ) -> Self {
        Self {
            targets,
            base_config,
            last_hit: StdMutex::new(None),
            last_attempt: StdMutex::new(None),
            responses_override: Some(responses_override),
        }
    }

    /// 最近一次成功 stream 的命中元数据。
    pub fn last_hit_meta(&self) -> Option<ActiveTargetMeta> {
        self.last_hit.lock().ok().and_then(|g| g.clone())
    }

    /// 最近一次实际发起请求的目标，失败时也可用。
    pub fn last_attempt_meta(&self) -> Option<ActiveTargetMeta> {
        self.last_attempt.lock().ok().and_then(|g| g.clone())
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

    /// 使用稳定指令采样，同时保持持久化的角色上下文历史独立分离。
    /// 原生工具 schema 保留在独立的 `tools` 参数中。
    pub(crate) async fn stream_responses_with_contract(
        &self,
        prompt: &crate::prompt::PromptContract,
        prompt_context: &[crate::prompt::context_state::PromptContextEvent],
        history: &[agent_protocol::ResponseItem],
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream> {
        let input = crate::prompt::messages::to_response_items_with_context_history(
            prompt_context,
            history,
        );
        self.stream_response(prompt.base_instructions.clone(), input, tools)
            .await
    }
}

#[async_trait]
impl StreamingResponses for ProviderStreamer {
    async fn stream_response(
        &self,
        instructions: String,
        input: Vec<agent_protocol::ResponseItem>,
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream> {
        let config = self.base_config.clone();

        if let Ok(mut guard) = self.last_attempt.lock() {
            *guard = self.targets.first().map(ActiveTargetMeta::from_target);
        }

        let (stream, meta) = if let Some(ref responses_fn) = self.responses_override {
            let stream = responses_fn(
                ResponsesOverrideInput {
                    instructions,
                    items: input,
                },
                tools,
                config.clone(),
            )
            .await?;
            let meta = ActiveTargetMeta {
                provider_id: self
                    .targets
                    .first()
                    .map(|t| t.provider_id.clone())
                    .unwrap_or_default(),
                backend_id: self
                    .targets
                    .first()
                    .map(|t| t.backend_id.clone())
                    .unwrap_or_default(),
                model: self
                    .targets
                    .first()
                    .map(|t| t.model.clone())
                    .unwrap_or_default(),
                base_url: self
                    .targets
                    .first()
                    .map(|t| t.base_url.clone())
                    .unwrap_or_default(),
            };
            (stream, meta)
        } else {
            try_stream_responses_with_fallback(
                &self.targets,
                instructions,
                input,
                tools,
                &config,
                |from, to, err| {
                    if let Ok(mut guard) = self.last_attempt.lock() {
                        *guard = Some(ActiveTargetMeta::from_target(to));
                    }
                    tracing::warn!(
                        from_backend = %from.backend_id,
                        from_model = %from.model,
                        to_backend = %to.backend_id,
                        to_model = %to.model,
                        error = %err,
                        "Responses failover: switching target before first content"
                    );
                },
            )
            .await?
        };

        let tracked: CompletionStream =
            Box::pin(futures::stream::unfold(stream, |mut stream| async move {
                stream.next().await.map(|item| (item, stream))
            }));

        if let Ok(mut guard) = self.last_hit.lock() {
            *guard = Some(meta);
        }
        Ok(map_new_provider_stream(tracked))
    }
}
