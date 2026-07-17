//! Skills 工具：按名称加载已安装 Skill 的 `SKILL.md` 说明。
//!
//! 实际读取逻辑委托 [`skills::load_skill_by_name`]；本模块负责参数校验与结果拼装。

use std::fs;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `skills` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SkillsArgs {
    /// Skill 名称 / id（不可为空）。
    pub skill_id: String,
    /// 可选结构化输入，会附在返回文本的「调用输入」小节。
    #[serde(default)]
    pub input: Option<serde_json::Value>,
}

/// 向注册表登记 `skills` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "skills".to_string(),
        toolset: "skills".to_string(),
        description: "Load an installed skill by name. Call with name=\"skills\" and arguments.skill_id equal to the skill name (e.g. brainstorming). Do not use the skill name itself as the tool name. Returns SKILL.md instructions plus the skill root path and scripts list so you can run files with terminal/code_exec. Body capped at 64KiB."
            .to_string(),
        schema: schema_for_args::<SkillsArgs>(),
        check_fn: None,
        icon: "puzzle",
            ..ToolEntry::lifecycle_defaults()
    });
}

/// Skill 根目录：`SKILL.md` 所在目录。
fn skill_root(skill_md: &Path) -> PathBuf {
    skill_md
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| skill_md.to_path_buf())
}

/// 列出 `scripts/` 下相对路径（最多 40 条，浅层递归）。
fn list_script_rel_paths(root: &Path) -> Vec<String> {
    let scripts = root.join("scripts");
    if !scripts.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        if out.len() >= 40 {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if out.len() >= 40 {
                break;
            }
            let path = entry.path();
            if path.is_dir() {
                walk(&path, root, out);
                continue;
            }
            if !path.is_file() {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    walk(&scripts, root, &mut out);
    out
}

fn format_path_section(root: &Path, scripts: &[String]) -> String {
    let root_disp = home::display_user_path(root);
    let mut lines = vec![
        "## Skill 路径".to_string(),
        format!("root: `{root_disp}`"),
    ];
    if scripts.is_empty() {
        lines.push("scripts: （无 scripts/ 目录或为空）".into());
    } else {
        lines.push("scripts:".into());
        for rel in scripts {
            lines.push(format!("- `{rel}`"));
        }
        lines.push(format!(
            "运行示例: `cd \"{root_disp}\" && python scripts/<file.py>`（按实际脚本与解释器调整）"
        ));
    }
    lines.join("\n")
}

/// 加载 Skill 内容，并附上路径、脚本清单与调用输入。
///
/// # 错误
/// 参数无效、`skill_id` 为空，或 Skill 不存在 / 读取失败。
pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: SkillsArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("skills 参数无效: {e}"))?;
    let skill_id = parsed.skill_id.trim();
    if skill_id.is_empty() {
        anyhow::bail!("skills 需要 skill_id");
    }

    let loaded = skills::load_skill_by_name(skill_id)?;
    let root = skill_root(&loaded.path);
    let scripts = list_script_rel_paths(&root);
    let path_section = format_path_section(&root, &scripts);

    let input = parsed.input.unwrap_or(serde_json::json!({}));
    let input_str = serde_json::to_string_pretty(&input).unwrap_or_default();
    let input_capped = if input_str.len() > 4 * 1024 {
        common::truncate_tool_result(&input_str, 4 * 1024)
    } else {
        input_str
    };
    let body = format!(
        "# Skill: {}\n\n{}\n\n{}\n\n## 调用输入\n{}",
        loaded.metadata.name, path_section, loaded.content, input_capped
    );
    Ok(common::truncate_tool_result(
        &body,
        common::MAX_TOOL_RESULT_BYTES,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn list_script_rel_paths_finds_nested_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("scripts/nested")).unwrap();
        fs::write(root.join("scripts/run.py"), "print(1)\n").unwrap();
        fs::write(root.join("scripts/nested/helper.sh"), "echo hi\n").unwrap();
        let listed = list_script_rel_paths(root);
        assert!(listed.iter().any(|p| p == "scripts/run.py"));
        assert!(listed.iter().any(|p| p == "scripts/nested/helper.sh"));
    }

    #[test]
    fn format_path_section_mentions_root_and_scripts() {
        let text = format_path_section(
            Path::new("/tmp/skill-demo"),
            &["scripts/run.py".into()],
        );
        assert!(text.contains("## Skill 路径"));
        assert!(text.contains("scripts/run.py"));
        assert!(text.contains("运行示例"));
    }
}
