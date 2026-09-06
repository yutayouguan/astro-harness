//! 待办工具：生成/更新 checklist Markdown，并同步写入 JSON 元数据。
//!
//! 文件落在工作区 `plans/`，便于 UI 与后续回合引用。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// 清单条目（对象形式）。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TodoItemObject {
    pub text: String,
    /// 是否完成；省略表示未完成。
    #[serde(default)]
    pub done: Option<bool>,
}

/// 清单条目：纯字符串，或 `{ text, done }` 对象。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum TodoItem {
    /// 仅文本；默认为未完成。
    Text(String),
    /// 带完成状态的对象。
    Object(TodoItemObject),
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum TodoAction {
    /// 创建新清单（默认）。
    #[default]
    Create,
    /// 按 plan_id 更新已有清单。
    Update,
}

/// `todo` 工具的参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TodoArgs {
    /// 操作："create"（默认）或 "update"。
    #[serde(default)]
    pub action: TodoAction,
    /// 列表标题；默认 `Todo`。
    #[serde(default)]
    pub title: Option<String>,
    pub items: Vec<TodoItem>,
    /// 计划文件名前缀（如 "20260722-a1b2c3"），action=update 时使用。
    #[serde(default)]
    pub plan_id: Option<String>,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "todo".to_string(),
        toolset: "todo".to_string(),
        description: "Create or update a todo checklist. \
action=create (default): write a new checklist to workspace/plans/. \
action=update: update an existing checklist by plan_id (the file stem, e.g. \"20260722-a1b2c3\"); \
replaces all items with the provided list."
            .to_string(),
        schema: schema_for_args::<TodoArgs>(),
        check_fn: None,
        icon: "list-todo",
        ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["todo"],
    sync_ctx: dispatch,
    args: TodoArgs,
}

pub fn dispatch(ctx: &ToolContext<'_>, args: &TodoArgs) -> anyhow::Result<String> {
    match args.action {
        TodoAction::Create => dispatch_create(ctx, args),
        TodoAction::Update => dispatch_update(ctx, args),
    }
}

fn normalize_items(items: &[TodoItem]) -> (Vec<String>, Vec<serde_json::Value>) {
    let mut lines = Vec::new();
    let mut normalized = Vec::new();
    for item in items {
        let (text, done) = match item {
            TodoItem::Text(s) => (s.clone(), false),
            TodoItem::Object(o) => (o.text.clone(), o.done.unwrap_or(false)),
        };
        let mark = if done { "x" } else { " " };
        lines.push(format!("- [{mark}] {text}"));
        normalized.push(serde_json::json!({ "text": text, "done": done }));
    }
    (lines, normalized)
}

fn dispatch_create(ctx: &ToolContext<'_>, parsed: &TodoArgs) -> anyhow::Result<String> {
    let title = parsed.title.as_deref().unwrap_or("Todo");
    let (item_lines, normalized) = normalize_items(&parsed.items);

    let mut lines = vec![format!("# {title}"), String::new()];
    lines.extend(item_lines);

    let dir = ctx.workspace_dir.join("plans");
    std::fs::create_dir_all(&dir)?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let stem = format!("{}-{}", chrono::Local::now().format("%Y%m%d"), &id[..6]);
    let md_path = dir.join(format!("{stem}.md"));
    let json_path = dir.join(format!("{stem}.json"));
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

fn dispatch_update(ctx: &ToolContext<'_>, parsed: &TodoArgs) -> anyhow::Result<String> {
    let plan_id = parsed
        .plan_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("action=update 需要 plan_id"))?;

    let dir = ctx.workspace_dir.join("plans");
    let md_path = dir.join(format!("{plan_id}.md"));
    let json_path = dir.join(format!("{plan_id}.json"));
    if !md_path.exists() {
        anyhow::bail!("plan 不存在: {plan_id}");
    }

    let old_title = if json_path.exists() {
        let raw = std::fs::read_to_string(&json_path)?;
        serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|v| v.get("title").and_then(|t| t.as_str()).map(String::from))
    } else {
        None
    };
    let title = parsed
        .title
        .as_deref()
        .or(old_title.as_deref())
        .unwrap_or("Todo");

    let (item_lines, normalized) = normalize_items(&parsed.items);
    let mut lines = vec![format!("# {title}"), String::new()];
    lines.extend(item_lines);

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
    Ok(format!("{body}\n\n已更新: {}", md_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_items_mixed() {
        let items = vec![
            TodoItem::Text("task A".into()),
            TodoItem::Object(TodoItemObject {
                text: "task B".into(),
                done: Some(true),
            }),
        ];
        let (lines, json) = normalize_items(&items);
        assert_eq!(lines[0], "- [ ] task A");
        assert_eq!(lines[1], "- [x] task B");
        assert_eq!(json.len(), 2);
    }
}
