//! 委派工具：将子任务记录到工作区，供后续专用 Agent 跟进。
//!
//! 当前实现写入 `workspace/delegates/{id前缀}.json`，状态为 `queued`；
//! 真正调度子 Agent 由上层编排（见 `agent::multi_agent`）完成。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `delegate` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct DelegateArgs {
    /// 要委派的子任务描述（不可为空）。
    pub task: String,
    /// 专用 Agent 类型标识；缺省为 `general`。
    #[serde(default)]
    pub agent_type: Option<String>,
}

/// 向注册表登记 `delegate` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "delegate".to_string(),
        toolset: "delegate".to_string(),
        description: "PLAN ONLY: queue a delegate JSON under workspace/delegates/ (status=queued). Does NOT run a sub-agent. For real execution use orchestration_run."
            .to_string(),
        schema: schema_for_args::<DelegateArgs>(),
        check_fn: None,
        icon: "send",
    });
}

/// 在工作区创建委派记录 JSON，并返回可读摘要。
///
/// # 参数
/// - `ctx`：提供 `workspace_dir` 与 `session_id`
/// - `args`：需符合 [`DelegateArgs`]
///
/// # 错误
/// 参数无效、`task` 为空，或写文件失败。
pub fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: DelegateArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("delegate 参数无效: {e}"))?;
    let task = parsed.task.trim();
    if task.is_empty() {
        anyhow::bail!("delegate 需要 task");
    }
    let agent_type = parsed.agent_type.as_deref().unwrap_or("general");

    let dir = ctx.workspace_dir.join("delegates");
    std::fs::create_dir_all(&dir)?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let path = dir.join(format!("{}.json", &id[..8]));
    let record = serde_json::json!({
        "id": id,
        "session_id": ctx.session_id,
        "agent_type": agent_type,
        "task": task,
        "status": "queued",
        "created_at": chrono::Local::now().to_rfc3339(),
    });
    std::fs::write(&path, serde_json::to_string_pretty(&record)?)?;
    Ok(format!(
        "已委派子任务 id={} agent_type={agent_type}\n任务: {task}\n记录: {}",
        &id[..8],
        path.display()
    ))
}
