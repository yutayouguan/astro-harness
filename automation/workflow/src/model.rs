use std::collections::HashMap;

use chrono::Local;
use serde::{Deserialize, Serialize};

/// 一条完整工作流定义，持久化在 `~/.astro/workflows/workflows.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub ai_callable: bool,
    pub nodes: Vec<WorkflowNode>,
    pub edges: Vec<WorkflowEdge>,
    #[serde(default)]
    pub variables: HashMap<String, serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
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

/// 29 种节点类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    // 触发器 (3)
    ManualTrigger,
    ScheduledTrigger,
    WebhookTrigger,
    // AI (3)
    AiAgentTask,
    ParameterExtraction,
    QuestionClassification,
    // 多媒体生成 (5)
    ImageGeneration,
    VideoGeneration,
    MusicGeneration,
    TextToSpeech,
    SubtitleGeneration,
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
    // 动作 (5)
    HttpRequest,
    RunLoop,
    DelayWait,
    Output,
    AudioProcessing,
    // 自定义（引用已保存的工作流）
    CustomLoop,
}

impl NodeType {
    pub fn category(&self) -> NodeCategory {
        match self {
            Self::ManualTrigger | Self::ScheduledTrigger | Self::WebhookTrigger => {
                NodeCategory::Trigger
            }
            Self::AiAgentTask | Self::ParameterExtraction | Self::QuestionClassification => {
                NodeCategory::Ai
            }
            Self::ImageGeneration
            | Self::VideoGeneration
            | Self::MusicGeneration
            | Self::TextToSpeech
            | Self::SubtitleGeneration => NodeCategory::Media,
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
            | Self::AudioProcessing => NodeCategory::Action,
            Self::CustomLoop => NodeCategory::Action,
        }
    }

    pub fn default_label(&self) -> &'static str {
        match self {
            Self::ManualTrigger => "手动触发",
            Self::ScheduledTrigger => "定时触发",
            Self::WebhookTrigger => "Webhook 触发",
            Self::AiAgentTask => "AI 智能体任务",
            Self::ParameterExtraction => "参数提取",
            Self::QuestionClassification => "问题分类",
            Self::ImageGeneration => "生成图片",
            Self::VideoGeneration => "生成视频",
            Self::MusicGeneration => "生成音乐",
            Self::TextToSpeech => "文字转语音",
            Self::SubtitleGeneration => "字幕生成",
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
            ai_callable: false,
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
    }
}
