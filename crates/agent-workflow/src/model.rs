use std::collections::HashMap;

use chrono::Local;
use serde::{Deserialize, Serialize};

/// 一条完整工作流定义，持久化在 `~/.astro/automation/workflows/workflows.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub agent_tool: WorkflowAgentTool,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
    #[serde(default)]
    pub variables: HashMap<String, serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// Workflow 暴露给 Agent 时的工具契约。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowAgentTool {
    #[serde(default)]
    pub exposure: WorkflowToolExposure,
    /// Responses API namespace 下的子工具名；留空时由 workflow id 生成稳定名。
    #[serde(default)]
    pub name: String,
    /// 对模型暴露的 JSON Schema object，参数会原样作为 `trigger_input`。
    #[serde(default = "default_agent_tool_input_schema")]
    pub input_schema: serde_json::Value,
    #[serde(default)]
    pub output_description: String,
    #[serde(default)]
    pub examples: Vec<serde_json::Value>,
    #[serde(default)]
    pub confirmation: WorkflowToolConfirmation,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkflowAgentToolPatch {
    pub exposure: Option<WorkflowToolExposure>,
    pub name: Option<String>,
    pub input_schema: Option<serde_json::Value>,
    pub output_description: Option<String>,
    pub examples: Option<Vec<serde_json::Value>>,
    pub confirmation: Option<WorkflowToolConfirmation>,
}

impl WorkflowAgentTool {
    pub fn deferred() -> Self {
        Self {
            exposure: WorkflowToolExposure::Deferred,
            ..Self::default()
        }
    }

    pub fn apply_patch(&mut self, patch: WorkflowAgentToolPatch) {
        if let Some(exposure) = patch.exposure {
            self.exposure = exposure;
        }
        if let Some(name) = patch.name {
            self.name = name;
        }
        if let Some(input_schema) = patch.input_schema {
            self.input_schema = input_schema;
        }
        if let Some(output_description) = patch.output_description {
            self.output_description = output_description;
        }
        if let Some(examples) = patch.examples {
            self.examples = examples;
        }
        if let Some(confirmation) = patch.confirmation {
            self.confirmation = confirmation;
        }
    }
}

impl Workflow {
    /// 返回 Responses API `workflow` namespace 下的稳定子工具名。
    pub fn agent_tool_name(&self) -> String {
        let configured = self.agent_tool.name.trim();
        if !configured.is_empty() {
            return configured.to_string();
        }
        format!("run_{}", self.id.replace('-', ""))
    }

    pub fn agent_tool_description(&self) -> String {
        let mut description = if self.description.trim().is_empty() {
            format!("运行智能工作流「{}」。", self.name)
        } else {
            format!(
                "运行智能工作流「{}」：{}",
                self.name,
                self.description.trim()
            )
        };
        if !self.agent_tool.output_description.trim().is_empty() {
            description.push_str(" 输出：");
            description.push_str(self.agent_tool.output_description.trim());
        }
        if !self.agent_tool.examples.is_empty() {
            description.push_str(" 调用示例：");
            description.push_str(
                &serde_json::to_string(&self.agent_tool.examples).unwrap_or_else(|_| "[]".into()),
            );
        }
        description
    }
}

pub fn is_valid_agent_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn is_reserved_agent_tool_name(name: &str) -> bool {
    matches!(name, "get_run" | "cancel_run")
}

pub fn validate_agent_tool_input(
    workflow: &Workflow,
    input: &serde_json::Value,
) -> anyhow::Result<()> {
    let validator = jsonschema::validator_for(&workflow.agent_tool.input_schema)
        .map_err(|error| anyhow::anyhow!("Agent 工具 input_schema 无效: {error}"))?;
    let errors = validator
        .iter_errors(input)
        .take(8)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    anyhow::ensure!(
        errors.is_empty(),
        "workflow 输入不符合 JSON Schema: {}",
        errors.join("; ")
    );
    Ok(())
}

impl Default for WorkflowAgentTool {
    fn default() -> Self {
        Self {
            exposure: WorkflowToolExposure::Disabled,
            name: String::new(),
            input_schema: default_agent_tool_input_schema(),
            output_description: String::new(),
            examples: Vec::new(),
            confirmation: WorkflowToolConfirmation::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowToolExposure {
    #[default]
    Disabled,
    Deferred,
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowToolConfirmation {
    #[default]
    Auto,
    Always,
}

fn default_agent_tool_input_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {},
        "additionalProperties": true
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNode {
    pub id: String,
    pub node_type: NodeType,
    pub label: String,
    pub position: Position,
    #[serde(default = "default_node_config")]
    pub config: serde_json::Value,
    #[serde(default)]
    pub disabled: bool,
}

fn default_node_config() -> serde_json::Value {
    serde_json::Value::Object(Default::default())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEdge {
    pub id: String,
    pub source: String,
    #[serde(default)]
    pub source_handle: Option<String>,
    pub target: String,
    #[serde(default)]
    pub target_handle: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    pub y: f64,
}

/// 43 种节点类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    // 触发器 (5)
    ManualTrigger,
    ScheduledTrigger,
    WebhookTrigger,
    EmailTrigger,
    FileWatchTrigger,
    // AI (7)
    AiAgentTask,
    ParameterExtraction,
    QuestionClassification,
    KnowledgeRetrieval,
    Summarization,
    SentimentAnalysis,
    DocumentUnderstanding,
    VisionUnderstanding,
    // 多媒体生成 (9)
    ImageGeneration,
    VideoGeneration,
    MusicGeneration,
    TextToSpeech,
    SubtitleGeneration,
    VoiceClone,
    SpeechToText,
    ImageEdit,
    Translation,
    // 流程控制 (6)
    Conditional,
    MultiBranch,
    Filter,
    Merge,
    Loop,
    HumanApproval,
    // 数据处理 (7)
    SetFields,
    FormatText,
    Json,
    Code,
    Sort,
    Slice,
    Aggregate,
    // 动作 (7)
    HttpRequest,
    RunLoop,
    DelayWait,
    Output,
    AudioProcessing,
    SendNotification,
    FileIo,
    // 自定义（引用已保存的工作流）
    CustomLoop,
}

impl NodeType {
    pub fn category(&self) -> NodeCategory {
        match self {
            Self::ManualTrigger
            | Self::ScheduledTrigger
            | Self::WebhookTrigger
            | Self::EmailTrigger
            | Self::FileWatchTrigger => NodeCategory::Trigger,
            Self::AiAgentTask
            | Self::ParameterExtraction
            | Self::QuestionClassification
            | Self::KnowledgeRetrieval
            | Self::Summarization
            | Self::SentimentAnalysis
            | Self::DocumentUnderstanding
            | Self::VisionUnderstanding => NodeCategory::Ai,
            Self::ImageGeneration
            | Self::VideoGeneration
            | Self::MusicGeneration
            | Self::TextToSpeech
            | Self::SubtitleGeneration
            | Self::VoiceClone
            | Self::SpeechToText
            | Self::ImageEdit
            | Self::Translation => NodeCategory::Media,
            Self::Conditional
            | Self::MultiBranch
            | Self::Filter
            | Self::Merge
            | Self::Loop
            | Self::HumanApproval => NodeCategory::FlowControl,
            Self::SetFields
            | Self::FormatText
            | Self::Json
            | Self::Code
            | Self::Sort
            | Self::Slice
            | Self::Aggregate => NodeCategory::DataProcessing,
            Self::HttpRequest
            | Self::RunLoop
            | Self::DelayWait
            | Self::Output
            | Self::AudioProcessing
            | Self::SendNotification
            | Self::FileIo
            | Self::CustomLoop => NodeCategory::Action,
        }
    }

    pub fn default_label(&self) -> &'static str {
        match self {
            Self::ManualTrigger => "手动触发",
            Self::ScheduledTrigger => "定时触发",
            Self::WebhookTrigger => "Webhook 触发",
            Self::EmailTrigger => "邮件触发",
            Self::FileWatchTrigger => "文件监控",
            Self::AiAgentTask => "AI 智能体任务",
            Self::ParameterExtraction => "参数提取",
            Self::QuestionClassification => "问题分类",
            Self::KnowledgeRetrieval => "知识检索",
            Self::Summarization => "文本摘要",
            Self::SentimentAnalysis => "情感分析",
            Self::DocumentUnderstanding => "文档理解",
            Self::VisionUnderstanding => "图片理解",
            Self::ImageGeneration => "生成图片",
            Self::VideoGeneration => "生成视频",
            Self::MusicGeneration => "生成音乐",
            Self::TextToSpeech => "文字转语音",
            Self::SubtitleGeneration => "字幕生成",
            Self::VoiceClone => "声音克隆",
            Self::SpeechToText => "语音识别",
            Self::ImageEdit => "图片编辑",
            Self::Translation => "翻译",
            Self::Conditional => "条件判断",
            Self::MultiBranch => "多路分支",
            Self::Filter => "过滤",
            Self::Merge => "合并",
            Self::Loop => "循环",
            Self::HumanApproval => "人工审批",
            Self::SetFields => "设置字段",
            Self::FormatText => "格式化文本",
            Self::Json => "JSON",
            Self::Code => "代码",
            Self::Sort => "排序",
            Self::Slice => "截取",
            Self::Aggregate => "聚合",
            Self::HttpRequest => "HTTP 请求",
            Self::RunLoop => "运行 Loop",
            Self::DelayWait => "延时等待",
            Self::Output => "输出",
            Self::AudioProcessing => "音频处理",
            Self::SendNotification => "发送通知",
            Self::FileIo => "文件读写",
            Self::CustomLoop => "自定义 Loop",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeCategory {
    Trigger,
    Ai,
    Media,
    FlowControl,
    DataProcessing,
    Action,
}

/// 创建工作流的输入
pub struct NewWorkflow {
    pub name: String,
    pub description: String,
}

impl Workflow {
    pub fn new(input: NewWorkflow) -> Self {
        let now = Local::now().to_rfc3339();
        let id = uuid::Uuid::new_v4().to_string();
        let trigger_id = uuid::Uuid::new_v4().to_string();
        Self {
            id,
            name: input.name,
            description: input.description,
            enabled: false,
            agent_tool: WorkflowAgentTool::deferred(),
            nodes: vec![WorkflowNode {
                id: trigger_id,
                node_type: NodeType::ManualTrigger,
                label: "手动触发".to_string(),
                position: Position { x: 200.0, y: 300.0 },
                config: serde_json::json!({}),
                disabled: false,
            }],
            edges: vec![],
            variables: HashMap::new(),
            created_at: now.clone(),
            updated_at: now,
            icon: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_type_roundtrip() {
        let nt = NodeType::ImageGeneration;
        let json = serde_json::to_string(&nt).unwrap();
        assert_eq!(json, "\"image_generation\"");
        let back: NodeType = serde_json::from_str(&json).unwrap();
        assert_eq!(back, nt);
    }

    #[test]
    fn workflow_serialize() {
        let wf = Workflow::new(NewWorkflow {
            name: "test".into(),
            description: "".into(),
        });
        let json = serde_json::to_string(&wf).unwrap();
        let back: Workflow = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, wf.id);
        assert_eq!(back.nodes.len(), 1);
        assert_eq!(back.agent_tool.exposure, WorkflowToolExposure::Deferred);
    }

    #[test]
    fn missing_agent_tool_defaults_to_disabled() {
        let raw = r#"{
            "id":"legacy",
            "name":"legacy",
            "description":"",
            "enabled":true,
            "nodes":[],
            "edges":[],
            "variables":{},
            "created_at":"now",
            "updated_at":"now"
        }"#;
        let workflow: Workflow = serde_json::from_str(raw).unwrap();
        assert_eq!(workflow.agent_tool.exposure, WorkflowToolExposure::Disabled);
    }

    #[test]
    fn validates_agent_tool_input_against_declared_schema() {
        let mut workflow = Workflow::new(NewWorkflow {
            name: "schema".into(),
            description: String::new(),
        });
        workflow.agent_tool.input_schema = serde_json::json!({
            "type": "object",
            "properties": {"topic": {"type": "string"}},
            "required": ["topic"],
            "additionalProperties": false
        });

        validate_agent_tool_input(&workflow, &serde_json::json!({"topic": "Astro"})).unwrap();
        assert!(validate_agent_tool_input(&workflow, &serde_json::json!({})).is_err());
        assert!(validate_agent_tool_input(&workflow, &serde_json::json!({"topic": 42})).is_err());
    }
}
