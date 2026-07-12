//! 多代理协作工具：为同一目标规划多个角色子代理。
//!
//! 当前写入 `workspace/multi_agent/{id}.json` 计划清单（状态 `planned`），
//! 不在此模块内真正并行执行子 Agent。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `multi_agent` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MultiAgentArgs {
    /// 总体目标（不可为空）。
    pub goal: String,
    /// 各子代理的角色描述列表。
    pub agents: Vec<String>,
}

/// 向注册表登记 `multi_agent` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "multi_agent".to_string(),
        toolset: "multi_agent".to_string(),
        description: "Coordinate multiple sub-agents toward a goal (writes a planned JSON checklist only). Prefer orchestration_run for real async serial execution."
            .to_string(),
        schema: schema_for_args::<MultiAgentArgs>(),
        check_fn: None,
        icon: "users",
    });
}

/// 生成多代理计划文件，并为每个角色写入 `planned` 状态条目。
///
/// # 错误
/// `goal` 为空、参数反序列化失败，或写文件失败。
pub fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: MultiAgentArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("multi_agent 参数无效: {e}"))?;
    let goal = parsed.goal.trim();
    if goal.is_empty() {
        anyhow::bail!("multi_agent 需要 goal");
    }

    let dir = ctx.workspace_dir.join("multi_agent");
    std::fs::create_dir_all(&dir)?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let mut results = Vec::new();
    for (i, role) in parsed.agents.iter().enumerate() {
        results.push(serde_json::json!({
            "index": i,
            "role": role,
            "status": "planned",
            "note": format!("子代理将处理目标的一部分: {role}")
        }));
    }
    let record = serde_json::json!({
        "id": id,
        "session_id": ctx.session_id,
        "goal": goal,
        "agents": results,
        "created_at": chrono::Local::now().to_rfc3339(),
    });
    let path = dir.join(format!("{}.json", &id[..8]));
    std::fs::write(&path, serde_json::to_string_pretty(&record)?)?;
    Ok(format!(
        "已创建多代理计划 id={}\ngoal={goal}\nagents={}\n记录: {}",
        &id[..8],
        parsed.agents.len(),
        path.display()
    ))
}
