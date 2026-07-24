use anyhow::{bail, Result};
use async_trait::async_trait;
use futures::StreamExt;

use providers::registry::ProviderRegistry;
use providers::trait_::ProviderConfig;
use providers::types::message::Message as ProviderMessage;
use providers::types::stream::StreamChunk;

use crate::engine::executor::{NodeExecutor, NodeResult};
use crate::engine::variables::VariableContext;
use crate::model::WorkflowNode;

fn build_provider_config(node: &WorkflowNode) -> Result<(String, ProviderConfig)> {
    let provider_id = node.config.get("provider_id").and_then(|v| v.as_str()).unwrap_or("openai");
    let model = node.config.get("model").and_then(|v| v.as_str()).unwrap_or("gpt-4o-mini");
    let temperature = node.config.get("temperature").and_then(|v| v.as_f64()).unwrap_or(0.7) as f32;
    let max_tokens = node.config.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(4096) as u32;

    let auth = providers::trait_::AuthKind::for_provider(provider_id);
    let api_key = if auth == providers::trait_::AuthKind::None {
        String::new()
    } else {
        providers::profile::read_env_api_key(provider_id).ok_or_else(|| {
            anyhow::anyhow!("未找到 {} 的 API Key 环境变量", provider_id)
        })?
    };
    let base_url = Some(providers::profile::default_base_for(provider_id).to_string())
        .filter(|s| !s.is_empty());

    Ok((provider_id.to_string(), ProviderConfig {
        api_key,
        base_url,
        model: model.to_string(),
        temperature,
        max_tokens,
        thinking_enabled: false,
        reasoning_effort: String::new(),
        additional_params: serde_json::json!({}),
        previous_interaction_id: None,
    }))
}

async fn one_shot_llm(provider_id: &str, config: &ProviderConfig, system: &str, user: &str) -> Result<String> {
    let registry = ProviderRegistry::new();
    let provider = registry.get(provider_id)
        .ok_or_else(|| anyhow::anyhow!("未找到 provider: {}", provider_id))?;

    let messages = vec![
        ProviderMessage::system(system),
        ProviderMessage::user_text(user),
    ];

    let mut stream = provider.chat_stream(messages, vec![], config).await?;
    let mut out = String::new();
    while let Some(item) = stream.next().await {
        let chunk = item?;
        if let StreamChunk::Text(token) = chunk {
            out.push_str(&token);
        }
    }
    Ok(out)
}

// ── AI Agent Task ───────────────────────────────────────────────────

pub struct AiAgentTaskExec;

#[async_trait]
impl NodeExecutor for AiAgentTaskExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let prompt_tpl = node.config.get("prompt_template").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = ctx.interpolate(prompt_tpl);
        let system_prompt = node.config.get("system_prompt").and_then(|v| v.as_str()).unwrap_or(
            "你是一个智能助手，请根据用户的指令完成任务。"
        );

        if prompt.trim().is_empty() {
            bail!("AI 节点的指令(prompt_template)为空");
        }

        let (provider_id, config) = build_provider_config(node)?;
        let response = one_shot_llm(&provider_id, &config, system_prompt, &prompt).await?;

        Ok(NodeResult::Success(serde_json::json!({
            "response": response,
            "model": config.model,
            "provider": provider_id,
        })))
    }
}

// ── Parameter Extraction ────────────────────────────────────────────

pub struct ParameterExtractionExec;

#[async_trait]
impl NodeExecutor for ParameterExtractionExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let prompt_tpl = node.config.get("prompt_template").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = ctx.interpolate(prompt_tpl);
        let fields = node.config.get("fields").and_then(|v| v.as_array());

        let field_desc = match fields {
            Some(arr) => arr.iter()
                .filter_map(|f| {
                    let name = f.get("name").and_then(|v| v.as_str())?;
                    let desc = f.get("description").and_then(|v| v.as_str()).unwrap_or("");
                    Some(format!("- {}: {}", name, desc))
                })
                .collect::<Vec<_>>()
                .join("\n"),
            None => String::new(),
        };

        let system = format!(
            "你是一个参数提取助手。请从用户输入中提取以下字段，以 JSON 对象格式返回。只输出 JSON，不要添加其他文字。\n\n需要提取的字段：\n{}",
            field_desc,
        );

        let (provider_id, config) = build_provider_config(node)?;
        let response = one_shot_llm(&provider_id, &config, &system, &prompt).await?;

        let parsed: serde_json::Value = serde_json::from_str(response.trim())
            .or_else(|_| {
                let cleaned = response.trim()
                    .trim_start_matches("```json").trim_start_matches("```")
                    .trim_end_matches("```").trim();
                serde_json::from_str(cleaned)
            })
            .unwrap_or(serde_json::json!({ "raw_response": response }));

        Ok(NodeResult::Success(parsed))
    }
}

// ── Question Classification ─────────────────────────────────────────

pub struct QuestionClassificationExec;

#[async_trait]
impl NodeExecutor for QuestionClassificationExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let prompt_tpl = node.config.get("prompt_template").and_then(|v| v.as_str()).unwrap_or("");
        let prompt = ctx.interpolate(prompt_tpl);
        let classes = node.config.get("classes").and_then(|v| v.as_array());

        let classes = match classes {
            Some(arr) if !arr.is_empty() => arr,
            _ => bail!("问题分类节点未配置分类列表(classes)"),
        };

        let class_desc = classes.iter()
            .filter_map(|c| {
                let id = c.get("id").and_then(|v| v.as_str())?;
                let desc = c.get("description").and_then(|v| v.as_str()).unwrap_or(id);
                Some(format!("- {} (id={})", desc, id))
            })
            .collect::<Vec<_>>()
            .join("\n");

        let class_ids: Vec<&str> = classes.iter()
            .filter_map(|c| c.get("id").and_then(|v| v.as_str()))
            .collect();

        let system = format!(
            "你是一个分类助手。根据用户输入，判断它属于以下哪个分类。只输出对应的 id，不要输出其他内容。\n\n可选分类：\n{}",
            class_desc,
        );

        let (provider_id, config) = build_provider_config(node)?;
        let response = one_shot_llm(&provider_id, &config, &system, &prompt).await?;
        let chosen_id = response.trim();

        let matched_id = if class_ids.contains(&chosen_id) {
            chosen_id.to_string()
        } else {
            class_ids.first().map(|s| s.to_string()).unwrap_or_default()
        };

        Ok(NodeResult::Branch(vec![matched_id]))
    }
}
