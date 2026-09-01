use anyhow::{bail, Result};
use async_trait::async_trait;
use futures::StreamExt;

use providers::types::request_content::{ChatCompletionMessage, UserContent};
use providers::types::stream::StreamChunk;
use providers::ProviderConfig;

use crate::engine::executor::{NodeExecutor, NodeResult};
use crate::engine::variables::VariableContext;
use crate::model::WorkflowNode;

fn build_provider_config(
    node: &WorkflowNode,
    ctx: &VariableContext,
) -> Result<(String, ProviderConfig)> {
    let provider_id = node
        .config
        .get("provider_id")
        .and_then(|v| v.as_str())
        .unwrap_or("openai");
    let model = node
        .config
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("gpt-4o-mini");
    let temperature = node
        .config
        .get("temperature")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.7) as f32;
    let max_tokens = node
        .config
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(4096) as u32;

    if let Some(runtime) = ctx.provider_config(provider_id) {
        let mut config = runtime.config.clone();
        if !model.trim().is_empty() {
            config.model = model.to_string();
        }
        config.temperature = temperature;
        config.max_tokens = max_tokens;
        return Ok((runtime.backend_id.clone(), config));
    }

    let auth = providers::AuthKind::for_provider(provider_id);
    let api_key = if auth == providers::AuthKind::None {
        String::new()
    } else {
        providers::profile::read_env_api_key(provider_id)
            .ok_or_else(|| anyhow::anyhow!("未找到 {} 的 API Key 环境变量", provider_id))?
    };
    let base_url = Some(providers::profile::default_base_for(provider_id).to_string())
        .filter(|s| !s.is_empty());

    Ok((
        provider_id.to_string(),
        ProviderConfig {
            api_key,
            base_url,
            model: model.to_string(),
            temperature,
            max_tokens,
            thinking_enabled: false,
            reasoning_effort: String::new(),
            additional_params: serde_json::json!({}),
            previous_interaction_id: None,
            api_mode: String::new(),
        },
    ))
}

pub async fn one_shot_llm(
    provider_id: &str,
    config: &ProviderConfig,
    system: &str,
    user: &str,
) -> Result<String> {
    let messages = vec![
        ChatCompletionMessage::system(system),
        ChatCompletionMessage::user_text(user),
    ];

    let mut stream =
        providers::dispatch::chat_stream(provider_id, messages, vec![], config).await?;
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
        let prompt_tpl = node
            .config
            .get("prompt_template")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let prompt = ctx.interpolate(prompt_tpl);
        let system_prompt = node
            .config
            .get("system_prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("你是一个智能助手，请根据用户的指令完成任务。");

        if prompt.trim().is_empty() {
            bail!("AI 节点的指令(prompt_template)为空");
        }

        let (provider_id, config) = build_provider_config(node, ctx)?;
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
        let prompt_tpl = node
            .config
            .get("prompt_template")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let prompt = ctx.interpolate(prompt_tpl);
        let fields = node.config.get("fields").and_then(|v| v.as_array());

        let field_desc = match fields {
            Some(arr) => arr
                .iter()
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

        let (provider_id, config) = build_provider_config(node, ctx)?;
        let response = one_shot_llm(&provider_id, &config, &system, &prompt).await?;

        let parsed: serde_json::Value = serde_json::from_str(response.trim())
            .or_else(|_| {
                let cleaned = response
                    .trim()
                    .trim_start_matches("```json")
                    .trim_start_matches("```")
                    .trim_end_matches("```")
                    .trim();
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
        let prompt_tpl = node
            .config
            .get("prompt_template")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let prompt = ctx.interpolate(prompt_tpl);
        let classes = node.config.get("classes").and_then(|v| v.as_array());

        let classes = match classes {
            Some(arr) if !arr.is_empty() => arr,
            _ => bail!("问题分类节点未配置分类列表(classes)"),
        };

        let class_desc = classes
            .iter()
            .filter_map(|c| {
                let id = c.get("id").and_then(|v| v.as_str())?;
                let desc = c.get("description").and_then(|v| v.as_str()).unwrap_or(id);
                Some(format!("- {} (id={})", desc, id))
            })
            .collect::<Vec<_>>()
            .join("\n");

        let class_ids: Vec<&str> = classes
            .iter()
            .filter_map(|c| c.get("id").and_then(|v| v.as_str()))
            .collect();

        let system = format!(
            "你是一个分类助手。根据用户输入，判断它属于以下哪个分类。只输出对应的 id，不要输出其他内容。\n\n可选分类：\n{}",
            class_desc,
        );

        let (provider_id, config) = build_provider_config(node, ctx)?;
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

// ── Knowledge Retrieval ────────────────────────────────────────────

pub struct KnowledgeRetrievalExec;

#[async_trait]
impl NodeExecutor for KnowledgeRetrievalExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let query_tpl = node
            .config
            .get("query_template")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let query = ctx.interpolate(query_tpl);
        let knowledge_path = node
            .config
            .get("knowledge_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let top_k = node
            .config
            .get("top_k")
            .and_then(|v| v.as_u64())
            .unwrap_or(5);

        if query.trim().is_empty() {
            bail!("知识检索节点的查询(query_template)为空");
        }

        // 暂用 LLM 模拟检索——后续接入 embedding + 向量数据库
        let system = format!(
            "你是一个知识检索助手。用户给出查询，请根据你的知识给出最相关的 {} 条结果。\
             知识来源参考: {}。以 JSON 数组返回，每条包含 content 和 relevance 字段。只输出 JSON。",
            top_k, knowledge_path,
        );
        let (provider_id, config) = build_provider_config(node, ctx)?;
        let response = one_shot_llm(&provider_id, &config, &system, &query).await?;

        let parsed: serde_json::Value = serde_json::from_str(response.trim())
            .unwrap_or(serde_json::json!({ "results": response }));
        Ok(NodeResult::Success(parsed))
    }
}

// ── Summarization ──────────────────────────────────────────────────

pub struct SummarizationExec;

#[async_trait]
impl NodeExecutor for SummarizationExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let text_tpl = node
            .config
            .get("text_template")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let text = ctx.interpolate(text_tpl);
        let style = node
            .config
            .get("style")
            .and_then(|v| v.as_str())
            .unwrap_or("concise");
        let max_len = node
            .config
            .get("max_length")
            .and_then(|v| v.as_u64())
            .unwrap_or(200);

        if text.trim().is_empty() {
            bail!("文本摘要节点的输入文本为空");
        }

        let system = format!(
            "你是一个文本摘要助手。请用「{}」风格对用户输入进行摘要，控制在 {} 字以内。只输出摘要文本。",
            style, max_len,
        );
        let (provider_id, config) = build_provider_config(node, ctx)?;
        let response = one_shot_llm(&provider_id, &config, &system, &text).await?;

        Ok(NodeResult::Success(serde_json::json!({
            "summary": response.trim(),
            "style": style,
        })))
    }
}

// ── Sentiment Analysis ─────────────────────────────────────────────

pub struct SentimentAnalysisExec;

#[async_trait]
impl NodeExecutor for SentimentAnalysisExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let text_tpl = node
            .config
            .get("text_template")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let text = ctx.interpolate(text_tpl);
        let custom_labels = node
            .config
            .get("custom_labels")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if text.trim().is_empty() {
            bail!("情感分析节点的输入文本为空");
        }

        let labels = if custom_labels.is_empty() {
            r#"["positive", "negative", "neutral"]"#.to_string()
        } else {
            custom_labels.to_string()
        };

        let system = format!(
            "你是一个情感分析助手。分析用户输入的情感倾向，从以下标签中选择最匹配的。\
             可选标签: {}。以 JSON 返回 {{\"label\": \"...\", \"confidence\": 0.0~1.0}}。只输出 JSON。",
            labels,
        );
        let (provider_id, config) = build_provider_config(node, ctx)?;
        let response = one_shot_llm(&provider_id, &config, &system, &text).await?;

        let parsed: serde_json::Value = serde_json::from_str(response.trim())
            .unwrap_or(serde_json::json!({ "label": response.trim(), "confidence": 1.0 }));
        Ok(NodeResult::Success(parsed))
    }
}

// ── Document Understanding ─────────────────────────────────────────

pub struct DocumentUnderstandingExec;

#[async_trait]
impl NodeExecutor for DocumentUnderstandingExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let task = node
            .config
            .get("task")
            .and_then(|v| v.as_str())
            .unwrap_or("ocr");
        let input_path = node
            .config
            .get("input_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let input_path = ctx.interpolate(input_path);
        let prompt_tpl = node
            .config
            .get("prompt_template")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let prompt = ctx.interpolate(prompt_tpl);

        let system = format!(
            "你是一个文档理解助手。任务类型: {}。用户将提供文档路径或内容，请完成相应的提取/分析任务。以 JSON 格式返回结果。",
            task,
        );
        let user_msg = if prompt.is_empty() {
            format!("请处理文档: {}", input_path)
        } else {
            format!("文档: {}\n\n指令: {}", input_path, prompt)
        };

        let (provider_id, config) = build_provider_config(node, ctx)?;
        let response = one_shot_llm(&provider_id, &config, &system, &user_msg).await?;

        let parsed: serde_json::Value = serde_json::from_str(response.trim())
            .unwrap_or(serde_json::json!({ "result": response.trim() }));
        Ok(NodeResult::Success(parsed))
    }
}

// ── Vision Understanding ───────────────────────────────────────────

pub struct VisionUnderstandingExec;

#[async_trait]
impl NodeExecutor for VisionUnderstandingExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let input_type = node
            .config
            .get("input_type")
            .and_then(|v| v.as_str())
            .unwrap_or("file");
        let input_path = node
            .config
            .get("input_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let input_path = ctx.interpolate(input_path);
        let prompt_tpl = node
            .config
            .get("prompt_template")
            .and_then(|v| v.as_str())
            .unwrap_or("描述这张图片");
        let prompt = ctx.interpolate(prompt_tpl);

        if input_path.trim().is_empty() {
            bail!("图片理解节点的输入路径为空");
        }

        let image_url = if input_type == "url" {
            input_path.clone()
        } else {
            let data = std::fs::read(&input_path)
                .map_err(|e| anyhow::anyhow!("读取图片失败 {}: {}", input_path, e))?;
            let mime = if input_path.ends_with(".png") {
                "image/png"
            } else if input_path.ends_with(".webp") {
                "image/webp"
            } else if input_path.ends_with(".gif") {
                "image/gif"
            } else {
                "image/jpeg"
            };
            let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data);
            format!("data:{};base64,{}", mime, b64)
        };

        let (provider_id, config) = build_provider_config(node, ctx)?;
        let messages = vec![
            ChatCompletionMessage::system("你是一个视觉理解助手。根据用户提示分析图片内容。"),
            ChatCompletionMessage::user(vec![
                UserContent::Image { url: image_url },
                UserContent::Text { text: prompt },
            ]),
        ];

        let mut stream =
            providers::dispatch::chat_stream(&provider_id, messages, vec![], &config).await?;
        let mut out = String::new();
        while let Some(item) = stream.next().await {
            let chunk = item?;
            if let StreamChunk::Text(token) = chunk {
                out.push_str(&token);
            }
        }

        Ok(NodeResult::Success(serde_json::json!({
            "description": out.trim(),
            "input_path": input_path,
            "provider": provider_id,
        })))
    }
}
