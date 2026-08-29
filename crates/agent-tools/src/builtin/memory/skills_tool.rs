//! Skills 工具：list / view(load) / curate / manage（create、update、patch、delete）。
//!
//! 加载逻辑委托 [`skills::load_skill_by_name`]；列表用 [`skills::list_enabled_for_prompt`]；
//! manage 写入当前 Agent 的 `workspace/skills/`（[`skills::install::agent_skills_dir`]）。

use std::fs;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `skills` 工具的参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SkillsArgs {
    /// 操作：`load` / `view`（默认）、`list`、`curate`、`manage`。
    #[serde(default)]
    pub action: Option<String>,
    /// Skill 名称 / id。`load`/`view`/`manage` 必填；`list`/`curate` 可选。
    #[serde(default)]
    pub skill_id: Option<String>,
    /// `manage` 子操作：`create` | `update` | `patch` | `delete`。
    #[serde(default)]
    pub manage_action: Option<String>,
    /// `manage` create/update：SKILL.md 正文（可包含 YAML frontmatter）。
    #[serde(default)]
    pub content: Option<String>,
    /// `manage` create：frontmatter 的 description（若 content 已含 frontmatter 则可选）。
    #[serde(default)]
    pub description: Option<String>,
    /// `manage` patch：要替换的文本（须在 SKILL.md 中唯一出现）。
    #[serde(default)]
    pub old_string: Option<String>,
    #[serde(default)]
    pub new_string: Option<String>,
    /// 可选的结构化输入，附加在 `load`/`view` 的"调用输入"区段下。
    #[serde(default)]
    pub input: Option<serde_json::Value>,
}

/// 向注册表登记 `skills` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "skills".to_string(),
        toolset: "skills".to_string(),
        description: "Skills hub: action=list|curate|load|view|manage. \
             list = enabled name+description; curate = usage + idle suggestions (never auto-deletes); \
             load|view (default) = SKILL.md + root/scripts; \
             manage with manage_action=create|update|patch|delete under the agent skills directory \
             (patch = unique old_string→new_string). \
             Call with name=\"skills\" and skill_id=skill name — never use the skill name as the tool name. \
             Body capped at 64KiB."
            .to_string(),
        schema: schema_for_args::<SkillsArgs>(),
        check_fn: None,
        icon: "puzzle",
        ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["skills"],
    sync_ctx: dispatch,
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
    let mut lines = vec!["## Skill 路径".to_string(), format!("root: `{root_disp}`")];
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

fn validate_skill_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty() {
        anyhow::bail!("skills 需要 skill_id");
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        anyhow::bail!("skill_id 不能包含路径分隔符");
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        anyhow::bail!("skill_id 仅允许字母、数字、.-_");
    }
    Ok(())
}

fn list_skills(config: &[(PathBuf, bool)]) -> String {
    let items = skills::list_enabled_for_prompt_with_config(config);
    if items.is_empty() {
        return "（无已启用技能）".to_string();
    }
    let mut lines = vec![format!("已启用技能（{}）:", items.len())];
    for (name, desc) in items {
        let d = if desc.trim().is_empty() {
            "（无描述）".to_string()
        } else {
            desc.replace('\n', " ")
        };
        lines.push(format!("- {name}: {d}"));
    }
    lines.join("\n")
}

fn load_skill(
    skill_id: &str,
    input: Option<serde_json::Value>,
    config: &[(PathBuf, bool)],
) -> anyhow::Result<String> {
    validate_skill_id(skill_id)?;
    let loaded = skills::load_skill_by_name_with_config(skill_id, config)?;
    let root = skill_root(&loaded.path);
    let scripts = list_script_rel_paths(&root);
    let path_section = format_path_section(&root, &scripts);

    let input = input.unwrap_or(serde_json::json!({}));
    let input_str = serde_json::to_string_pretty(&input).unwrap_or_default();
    let input_capped = if input_str.len() > 4 * 1024 {
        types::truncate_tool_result(&input_str, 4 * 1024)
    } else {
        input_str
    };
    let body = format!(
        "# Skill: {}\n\n{}\n\n{}\n\n## 调用输入\n{}",
        loaded.metadata.name, path_section, loaded.content, input_capped
    );
    Ok(types::truncate_tool_result(
        &body,
        types::MAX_TOOL_RESULT_BYTES,
    ))
}

fn resolve_agent_skill_md(skill_id: &str, skills_dir: &Path) -> anyhow::Result<PathBuf> {
    let dest = skills_dir.join(skill_id);
    let skill_md = if dest.join("SKILL.md").is_file() {
        dest.join("SKILL.md")
    } else {
        anyhow::bail!(
            "未找到 Agent skills 下的 SKILL.md: {}（patch/update 仅可改 Agent 目录）",
            home::display_user_path(&dest)
        );
    };
    let agent_root = skills_dir
        .canonicalize()
        .unwrap_or_else(|_| skills_dir.to_path_buf());
    let parent = skill_md
        .parent()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
        .unwrap_or_else(|| dest.clone());
    if !parent.starts_with(&agent_root) {
        anyhow::bail!("路径越界：拒绝修改 Agent skills 之外的文件");
    }
    Ok(skill_md)
}

fn patch_skill_md(path: &Path, rel_label: &str, old: &str, new: &str) -> anyhow::Result<String> {
    if old.is_empty() {
        anyhow::bail!("patch 的 old_string 不能为空");
    }
    let raw = fs::read(path)?;
    let text = std::str::from_utf8(&raw)
        .map_err(|_| anyhow::anyhow!("patch 仅支持 UTF-8: {rel_label}"))?;
    let matches = text.matches(old).count();
    match matches {
        0 => anyhow::bail!("patch 未找到 old_string（0 处匹配）: {rel_label}"),
        1 => {
            let updated = text.replacen(old, new, 1);
            fs::write(path, updated.as_bytes())?;
            Ok(format!("已 patch 技能 `{rel_label}`（1 处替换）"))
        }
        n => {
            anyhow::bail!("patch 的 old_string 不唯一（{n} 处匹配），请提供更长上下文: {rel_label}")
        }
    }
}

fn manage_skill(
    skill_id: &str,
    manage_action: &str,
    content: Option<&str>,
    description: Option<&str>,
    old_string: Option<&str>,
    new_string: Option<&str>,
) -> anyhow::Result<String> {
    validate_skill_id(skill_id)?;
    let op = manage_action.trim().to_lowercase();
    let skills_dir = skills::install::agent_skills_dir(None)?;
    let dest = skills_dir.join(skill_id);

    match op.as_str() {
        "create" => {
            if dest.exists() {
                anyhow::bail!("技能已存在: {skill_id}（用 manage_action=update 或 patch 修改）");
            }
            let body = content
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("manage create 需要 content（SKILL.md 正文）"))?;
            fs::create_dir_all(&dest)?;
            let md = if body.starts_with("---") {
                body.to_string()
            } else {
                let desc = description.unwrap_or("").trim().replace('"', "'");
                format!("---\nname: {skill_id}\ndescription: \"{desc}\"\n---\n\n{body}\n")
            };
            fs::write(dest.join("SKILL.md"), md.as_bytes())?;
            let _ = skills::set_enabled(skill_id, true);
            Ok(format!(
                "已创建技能 `{skill_id}`\n路径: {}",
                home::display_user_path(&dest)
            ))
        }
        "update" => {
            let body = content
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("manage update 需要 content"))?;
            let skill_md = if dest.join("SKILL.md").is_file() {
                dest.join("SKILL.md")
            } else {
                let loaded = skills::load_skill_by_name(skill_id).ok();
                if let Some(loaded) = loaded {
                    loaded.path
                } else if dest.exists() {
                    dest.join("SKILL.md")
                } else {
                    anyhow::bail!("未找到可更新的技能目录: {skill_id}");
                }
            };
            let agent_root = skills_dir
                .canonicalize()
                .unwrap_or_else(|_| skills_dir.clone());
            let parent = skill_md
                .parent()
                .map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
                .unwrap_or_else(|| dest.clone());
            if !parent.starts_with(&agent_root) {
                fs::create_dir_all(&dest)?;
                let target = dest.join("SKILL.md");
                fs::write(&target, body.as_bytes())?;
                let _ = skills::set_enabled(skill_id, true);
                return Ok(format!(
                    "已将 `{skill_id}` 写入 Agent skills（原路径不可直接改）\n路径: {}",
                    home::display_user_path(&dest)
                ));
            }
            if let Some(p) = skill_md.parent() {
                fs::create_dir_all(p)?;
            }
            fs::write(&skill_md, body.as_bytes())?;
            Ok(format!(
                "已更新技能 `{skill_id}`\n路径: {}",
                home::display_user_path(&skill_md)
            ))
        }
        "patch" => {
            let old = old_string
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("manage patch 需要 old_string"))?;
            let new = new_string.ok_or_else(|| anyhow::anyhow!("manage patch 需要 new_string"))?;
            let skill_md = resolve_agent_skill_md(skill_id, &skills_dir)?;
            patch_skill_md(&skill_md, skill_id, old, new)
        }
        "delete" => {
            if !dest.exists() {
                anyhow::bail!(
                    "仅可删除 Agent skills 目录下的技能；未找到: {}",
                    home::display_user_path(&dest)
                );
            }
            let canon = dest.canonicalize().unwrap_or_else(|_| dest.clone());
            let agent_root = skills_dir
                .canonicalize()
                .unwrap_or_else(|_| skills_dir.clone());
            if !canon.starts_with(&agent_root) || canon == agent_root {
                anyhow::bail!("拒绝删除 skills 根目录或越界路径");
            }
            if canon.is_dir() {
                fs::remove_dir_all(&canon)?;
            } else {
                fs::remove_file(&canon)?;
            }
            let _ = skills::set_enabled(skill_id, false);
            Ok(format!("已删除技能 `{skill_id}`"))
        }
        other => {
            anyhow::bail!("未知 manage_action: {other}（支持 create|update|patch|delete）")
        }
    }
}

/// 按 `action` 分发 skills 操作。
pub fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: SkillsArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("skills 参数无效: {e}"))?;

    let action = parsed
        .action
        .as_deref()
        .unwrap_or("load")
        .trim()
        .to_lowercase();

    match action.as_str() {
        "list" => Ok(list_skills(ctx.skill_config_overrides)),
        "curate" => {
            let days = memory::load_learning_config(&home::default_memory_dir()).unused_skill_days;
            Ok(skills::curate_report(days))
        }
        "load" | "view" => {
            let skill_id = parsed
                .skill_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("skills load/view 需要 skill_id"))?;
            load_skill(skill_id, parsed.input, ctx.skill_config_overrides)
        }
        "manage" => {
            let skill_id = parsed
                .skill_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("skills manage 需要 skill_id"))?;
            let manage_action = parsed
                .manage_action
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "skills manage 需要 manage_action（create|update|patch|delete）"
                    )
                })?;
            manage_skill(
                skill_id,
                manage_action,
                parsed.content.as_deref(),
                parsed.description.as_deref(),
                parsed.old_string.as_deref(),
                parsed.new_string.as_deref(),
            )
        }
        other => anyhow::bail!("未知 action: {other}（支持 list|curate|load|view|manage）"),
    }
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
        let text = format_path_section(Path::new("/tmp/skill-demo"), &["scripts/run.py".into()]);
        assert!(text.contains("## Skill 路径"));
        assert!(text.contains("scripts/run.py"));
        assert!(text.contains("运行示例"));
    }

    #[test]
    fn validate_skill_id_rejects_path() {
        assert!(validate_skill_id("../x").is_err());
        assert!(validate_skill_id("a/b").is_err());
        assert!(validate_skill_id("good-skill_1.0").is_ok());
    }

    #[test]
    fn manage_create_update_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());
        let create = manage_skill(
            "demo-tool-skill",
            "create",
            Some("# Hello\n\nDo the thing."),
            Some("A demo skill"),
            None,
            None,
        )
        .unwrap();
        assert!(create.contains("已创建"));
        let skills_dir = skills::install::agent_skills_dir(None).unwrap();
        let md = skills_dir.join("demo-tool-skill/SKILL.md");
        assert!(md.is_file());

        manage_skill(
            "demo-tool-skill",
            "update",
            Some("---\nname: demo-tool-skill\ndescription: updated\n---\n\n# Updated\n"),
            None,
            None,
            None,
        )
        .unwrap();
        assert!(fs::read_to_string(&md).unwrap().contains("# Updated"));

        let del = manage_skill("demo-tool-skill", "delete", None, None, None, None).unwrap();
        assert!(del.contains("已删除"));
        assert!(!skills_dir.join("demo-tool-skill").exists());
    }

    #[test]
    fn manage_patch_unique_and_failures() {
        let dir = tempfile::tempdir().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());
        manage_skill(
            "patch-demo",
            "create",
            Some("# A\n\nstep one\n\nstep two\n"),
            Some("patch demo"),
            None,
            None,
        )
        .unwrap();
        let out = manage_skill(
            "patch-demo",
            "patch",
            None,
            None,
            Some("step one"),
            Some("step ONE"),
        )
        .unwrap();
        assert!(out.contains("1 处替换"));
        let skills_dir = skills::install::agent_skills_dir(None).unwrap();
        let body = fs::read_to_string(skills_dir.join("patch-demo/SKILL.md")).unwrap();
        assert!(body.contains("step ONE"));
        assert!(!body.contains("step one"));

        let err0 = manage_skill(
            "patch-demo",
            "patch",
            None,
            None,
            Some("missing-marker"),
            Some("x"),
        )
        .unwrap_err();
        assert!(err0.to_string().contains("0 处匹配"));

        // make duplicate
        fs::write(skills_dir.join("patch-demo/SKILL.md"), "xx xx xx\n").unwrap();
        let err_n =
            manage_skill("patch-demo", "patch", None, None, Some("xx"), Some("yy")).unwrap_err();
        assert!(err_n.to_string().contains("不唯一"));
    }

    #[test]
    fn patch_skill_md_unit() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("SKILL.md");
        fs::write(&p, "hello world\n").unwrap();
        patch_skill_md(&p, "t", "hello", "hi").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "hi world\n");
        assert!(patch_skill_md(&p, "t", "nope", "x").is_err());
    }
}
