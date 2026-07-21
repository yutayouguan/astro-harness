//! 任务计划工具：生成 checklist Markdown，并同步写入 JSON 元数据。
//!
//! 文件落在工作区 `plans/`，便于 UI 与后续回合引用。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// Checklist item as an object.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PlanItemObject {
    pub text: String,
    /// Whether completed; omitted means incomplete.
    #[serde(default)]
    pub done: Option<bool>,
}

/// Checklist item: plain string, or `{ text, done }` object.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum PlanItem {
    /// Text only; defaults to incomplete.
    Text(String),
    /// Object with completion state.
    Object(PlanItemObject),
}

/// Arguments for the `task_plan` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TaskPlanArgs {
    /// Plan title; default `Task Plan`.
    #[serde(default)]
    pub title: Option<String>,
    pub items: Vec<PlanItem>,
}

/// 向注册表登记 `task_plan` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "task_plan".to_string(),
        toolset: "task_plan".to_string(),
        description: "Always creates a NEW structured task plan checklist (does not update an existing plan). Writes Markdown+JSON under workspace/plans/."
            .to_string(),
        schema: schema_for_args::<TaskPlanArgs>(),
        check_fn: None,
        icon: "list-todo",
            ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["task_plan"],
    sync_ctx: dispatch,
}

/// 规范化条目并写入 `.md` + `.json`，同时把 Markdown 正文返回给模型。
///
/// # 错误
/// 参数反序列化失败，或写文件失败。
pub fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: TaskPlanArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("task_plan 参数无效: {e}"))?;
    let title = parsed.title.as_deref().unwrap_or("Task Plan");

    let mut lines = vec![format!("# {title}"), String::new()];
    let mut normalized = Vec::new();
    for item in &parsed.items {
        let (text, done) = match item {
            PlanItem::Text(s) => (s.clone(), false),
            PlanItem::Object(o) => (o.text.clone(), o.done.unwrap_or(false)),
        };
        let mark = if done { "x" } else { " " };
        lines.push(format!("- [{mark}] {text}"));
        normalized.push(serde_json::json!({ "text": text, "done": done }));
    }

    let dir = ctx.workspace_dir.join("plans");
    std::fs::create_dir_all(&dir)?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let md_path = dir.join(format!(
        "{}-{}.md",
        chrono::Local::now().format("%Y%m%d"),
        &id[..6]
    ));
    let json_path = md_path.with_extension("json");
    let body = lines.join("\n");
    std::fs::write(&md_path, &body)?;
    std::fs::write(
        &json_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "title": title,
            "session_id": ctx.session_id,
            "items": normalized,
        }))?,
    )?;
    Ok(format!("{body}\n\n已保存: {}", md_path.display()))
}
