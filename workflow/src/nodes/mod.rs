pub mod trigger;
pub mod data;
pub mod control;
pub mod action;
pub mod ai;
pub mod ai_stub;
pub mod media;
pub mod media_stub;

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::engine::executor::NodeExecutor;
use crate::model::NodeType;

/// 获取全局共享的执行器注册表（只构建一次）
pub fn executor_registry() -> &'static HashMap<NodeType, Box<dyn NodeExecutor>> {
    static REGISTRY: OnceLock<HashMap<NodeType, Box<dyn NodeExecutor>>> = OnceLock::new();
    REGISTRY.get_or_init(build_executor_registry)
}

/// 构建全部节点类型的执行器注册表
pub fn build_executor_registry() -> HashMap<NodeType, Box<dyn NodeExecutor>> {
    let mut m: HashMap<NodeType, Box<dyn NodeExecutor>> = HashMap::new();

    // 触发器
    m.insert(NodeType::ManualTrigger, Box::new(trigger::ManualTriggerExec));
    m.insert(NodeType::ScheduledTrigger, Box::new(trigger::ScheduledTriggerExec));
    m.insert(NodeType::WebhookTrigger, Box::new(trigger::WebhookTriggerExec));

    // AI — 接入 providers crate 调用 LLM
    m.insert(NodeType::AiAgentTask, Box::new(ai::AiAgentTaskExec));
    m.insert(NodeType::ParameterExtraction, Box::new(ai::ParameterExtractionExec));
    m.insert(NodeType::QuestionClassification, Box::new(ai::QuestionClassificationExec));

    // 多媒体 — 图片生成已接入，其余待开发
    m.insert(NodeType::ImageGeneration, Box::new(media::ImageGenExec));
    m.insert(NodeType::VideoGeneration, Box::new(media::VideoGenExec));
    m.insert(NodeType::MusicGeneration, Box::new(media::MusicGenExec));
    m.insert(NodeType::TextToSpeech, Box::new(media::TtsExec));
    m.insert(NodeType::SubtitleGeneration, Box::new(media::SubtitleGenExec));

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
