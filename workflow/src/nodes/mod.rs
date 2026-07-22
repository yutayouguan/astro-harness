pub mod trigger;
pub mod data;
pub mod control;
pub mod action;
pub mod ai_stub;
pub mod media_stub;

use std::collections::HashMap;

use crate::engine::executor::NodeExecutor;
use crate::model::NodeType;

/// 构建全部节点类型的执行器注册表
pub fn build_executor_registry() -> HashMap<NodeType, Box<dyn NodeExecutor>> {
    let mut m: HashMap<NodeType, Box<dyn NodeExecutor>> = HashMap::new();

    // 触发器
    m.insert(NodeType::ManualTrigger, Box::new(trigger::ManualTriggerExec));
    m.insert(NodeType::ScheduledTrigger, Box::new(trigger::ScheduledTriggerExec));
    m.insert(NodeType::WebhookTrigger, Box::new(trigger::WebhookTriggerExec));

    // AI（桩实现，待 agent crate 接入）
    m.insert(NodeType::AiAgentTask, Box::new(ai_stub::AiAgentTaskExec));
    m.insert(NodeType::ParameterExtraction, Box::new(ai_stub::ParameterExtractionExec));
    m.insert(NodeType::QuestionClassification, Box::new(ai_stub::QuestionClassificationExec));

    // 多媒体（桩实现，待 providers crate 接入）
    m.insert(NodeType::ImageGeneration, Box::new(media_stub::ImageGenExec));
    m.insert(NodeType::VideoGeneration, Box::new(media_stub::VideoGenExec));
    m.insert(NodeType::MusicGeneration, Box::new(media_stub::MusicGenExec));
    m.insert(NodeType::TextToSpeech, Box::new(media_stub::TtsExec));
    m.insert(NodeType::SubtitleGeneration, Box::new(media_stub::SubtitleGenExec));

    // 流程控制
    m.insert(NodeType::Conditional, Box::new(control::ConditionalExec));
    m.insert(NodeType::MultiBranch, Box::new(control::MultiBranchExec));
    m.insert(NodeType::Filter, Box::new(control::FilterExec));
    m.insert(NodeType::Merge, Box::new(control::MergeExec));
    m.insert(NodeType::Loop, Box::new(control::LoopExec));
    m.insert(NodeType::HumanApproval, Box::new(control::HumanApprovalExec));

    // 数据处理
    m.insert(NodeType::SetFields, Box::new(data::SetFieldsExec));
    m.insert(NodeType::FormatText, Box::new(data::FormatTextExec));
    m.insert(NodeType::Json, Box::new(data::JsonExec));
    m.insert(NodeType::Code, Box::new(data::CodeExec));
    m.insert(NodeType::Sort, Box::new(data::SortExec));
    m.insert(NodeType::Slice, Box::new(data::SliceExec));
    m.insert(NodeType::Aggregate, Box::new(data::AggregateExec));

    // 动作
    m.insert(NodeType::HttpRequest, Box::new(action::HttpRequestExec));
    m.insert(NodeType::RunLoop, Box::new(action::RunLoopExec));
    m.insert(NodeType::DelayWait, Box::new(action::DelayWaitExec));
    m.insert(NodeType::Output, Box::new(action::OutputExec));
    m.insert(NodeType::AudioProcessing, Box::new(action::AudioProcessingExec));
    m.insert(NodeType::CustomLoop, Box::new(action::RunLoopExec));

    m
}
