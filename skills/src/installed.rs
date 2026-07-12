//! 本机 Skill 扫描、启用状态与按名称加载。

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};

use crate::models::InstalledSkill;
use crate::skill::{LoadedSkill, SkillMetadata};

/// 启用状态文件名（位于 Agent / 全局配置目录）。
const STATE_FILE: &str = "skills-enabled.json";

/// 解析本机 Astro 数据根目录。
fn memory_dir() -> PathBuf {
    std::env::var("ASTRO_MEMORY_DIR")
        .map(PathBuf::from)
        .or_else(|_| {
            std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .map(|h| PathBuf::from(h).join(".astro"))
        })
        .unwrap_or_else(|_| PathBuf::from(".astro"))
}

/// 规范化 Agent id：空→None，`default`→`workspace`。
fn normalize_agent_id(agent_id: Option<&str>) -> Option<String> {
    agent_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s == "default" {
                "workspace".to_string()
            } else {
                s.to_string()
            }
        })
}

/// Agent 工作区目录（`workspace` 或 `workspace-{id}`）。
fn agent_workspace(agent_id: &str) -> PathBuf {
    let base = memory_dir();
    if agent_id == "workspace" {
        base.join("workspace")
    } else {
        base.join(format!("workspace-{agent_id}"))
    }
}

/// 读取 `active-agent.json` 中的当前 Agent id。
fn active_agent_id() -> Option<String> {
    let base = memory_dir();
    fs::read_to_string(base.join("active-agent.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| {
            v.get("id")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
}

/// 启用状态文件路径（按 Agent 或全局）。
fn state_path_for(agent_id: Option<&str>) -> PathBuf {
    let base = memory_dir();
    match normalize_agent_id(agent_id) {
        Some(id) => base.join("agents").join(id).join(STATE_FILE),
        None => base.join(STATE_FILE),
    }
}

/// 加载技能启用表；Agent 级缺失时回退全局。
fn load_enabled_state(agent_id: Option<&str>) -> HashMap<String, bool> {
    let path = state_path_for(agent_id);
    let path = if !path.exists() && agent_id.is_some() {
        memory_dir().join(STATE_FILE)
    } else {
        path
    };
    if !path.exists() {
        return HashMap::new();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 持久化技能启用表；workspace Agent 同步写全局副本。
fn save_enabled_state(agent_id: Option<&str>, state: &HashMap<String, bool>) -> Result<()> {
    let path = state_path_for(agent_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(state)?;
    fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    if normalize_agent_id(agent_id).as_deref() == Some("workspace") {
        let global = memory_dir().join(STATE_FILE);
        let _ = fs::write(&global, serde_json::to_string_pretty(state)?);
    }
    Ok(())
}

/// Astro 管理范围：`~/.astro/skills` + 当前 Agent 工作区 skills
fn astro_skill_roots(agent_id: Option<&str>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let astro = memory_dir().join("skills");
    let _ = fs::create_dir_all(&astro);
    roots.push(astro);

    if let Some(id) = normalize_agent_id(agent_id) {
        let ws = agent_workspace(&id);
        roots.push(ws.join("skills"));
        roots.push(ws.join(".agents/skills"));
        roots.push(ws.join(".cursor/skills"));
    } else if let Ok(ws) = std::env::var("ASTRO_WORKSPACE") {
        let ws = PathBuf::from(ws);
        roots.push(ws.join("skills"));
        roots.push(ws.join(".agents/skills"));
        roots.push(ws.join(".cursor/skills"));
    } else {
        let default_ws = memory_dir().join("workspace");
        roots.push(default_ws.join("skills"));
    }

    roots.sort();
    roots.dedup();
    roots
}

/// 本机其它技能目录（Codex / Claude / Cursor 等），不含 Astro 数据根
fn machine_skill_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mem = memory_dir();

    if let Ok(mut dir) = std::env::current_dir() {
        for _ in 0..8 {
            for sub in [".agents/skills", ".cursor/skills"] {
                let p = dir.join(sub);
                if !p.starts_with(&mem) {
                    roots.push(p);
                }
            }
            if dir.parent().is_none() {
                break;
            }
            dir = dir.parent().unwrap().to_path_buf();
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let home = PathBuf::from(home);
        for sub in [
            ".agents/skills",
            ".cursor/skills",
            ".claude/skills",
            ".codex/skills",
        ] {
            let p = home.join(sub);
            if !p.starts_with(&mem) {
                roots.push(p);
            }
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

/// 解析 SKILL.md YAML frontmatter 为元数据。
fn parse_skill_frontmatter(content: &str) -> (String, String) {
    let mut name = String::new();
    let mut description = String::new();
    if let Some(rest) = content.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            for line in rest[..end].lines() {
                if let Some(v) = line.strip_prefix("name:") {
                    name = v.trim().trim_matches('"').to_string();
                } else if let Some(v) = line.strip_prefix("description:") {
                    description = v.trim().trim_matches('"').to_string();
                }
            }
        }
    }
    (name, description)
}

/// 比较两条路径是否同一位置（canonicalize 后）。
fn same_path(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

/// 判断技能目录是否已链接/位于目标 Agent 工作区。
fn is_linked_into_agent(agent_id: Option<&str>, skill_dir: &Path, skill_name: &str) -> bool {
    let Some(id) = normalize_agent_id(agent_id) else {
        return false;
    };
    let link = agent_workspace(&id).join("skills").join(skill_name);
    if !link.exists() {
        return false;
    }
    same_path(&link, skill_dir)
}

/// 扫描给定根目录列表下的已安装技能。
fn scan_roots(
    roots: &[PathBuf],
    agent_id: Option<&str>,
    scope: &str,
    state: &mut HashMap<String, bool>,
    dirty: &mut bool,
    persist_defaults: bool,
) -> Vec<InstalledSkill> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let source_dir = root.to_string_lossy().to_string();
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let skill_md = path.join("SKILL.md");
            if !skill_md.is_file() {
                continue;
            }
            let Ok(content) = fs::read_to_string(&skill_md) else {
                continue;
            };
            let folder = path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let (mut name, description) = parse_skill_frontmatter(&content);
            if name.is_empty() {
                name = folder.clone();
            }
            let id = format!("{source_dir}/{folder}");
            seen.insert(id.clone());

            let linked = if scope == "machine" {
                is_linked_into_agent(agent_id, &path, &name)
            } else {
                false
            };

            let enabled = if scope == "machine" {
                linked
            } else if let Some(v) = state.get(&id).copied() {
                v
            } else if persist_defaults {
                state.insert(id.clone(), true);
                *dirty = true;
                true
            } else {
                true
            };

            out.push(InstalledSkill {
                id,
                name,
                description,
                path: skill_md.to_string_lossy().to_string(),
                source_dir: source_dir.clone(),
                enabled,
                scope: scope.to_string(),
                linked,
            });
        }
    }

    if persist_defaults {
        let before = state.len();
        state.retain(|id, _| seen.contains(id) || !id_belongs_to_roots(id, roots));
        // Only drop keys that belonged to these roots but disappeared
        let _ = before;
    }

    out
}

/// 技能 id 是否落在指定根路径之下。
fn id_belongs_to_roots(id: &str, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| {
        let prefix = format!("{}/", r.to_string_lossy());
        id.starts_with(&prefix) || id == r.to_string_lossy().as_ref()
    })
}

/// 扫描 Astro 管理的技能根。
fn scan_astro(agent_id: Option<&str>) -> Vec<InstalledSkill> {
    let mut state = load_enabled_state(agent_id);
    let mut dirty = false;
    let roots = astro_skill_roots(agent_id);
    let mut out = scan_roots(&roots, agent_id, "astro", &mut state, &mut dirty, true);

    let seen: std::collections::HashSet<_> = out.iter().map(|s| s.id.clone()).collect();
    let before = state.len();
    state.retain(|id, _| {
        if id_belongs_to_roots(id, &roots) {
            seen.contains(id)
        } else {
            // keep machine / other keys untouched
            true
        }
    });
    if state.len() != before {
        dirty = true;
    }
    if dirty {
        let _ = save_enabled_state(agent_id, &state);
    }

    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// 扫描本机其它技能根（如 Claude 共享目录）。
fn scan_machine(agent_id: Option<&str>) -> Vec<InstalledSkill> {
    let roots = machine_skill_roots();
    let mut state = HashMap::new();
    let mut dirty = false;
    let mut out = scan_roots(
        &roots,
        agent_id,
        "machine",
        &mut state,
        &mut dirty,
        false,
    );
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// 扫描本机技能目录，合并启用状态；新发现的技能默认启用
pub fn list_installed() -> Vec<InstalledSkill> {
    list_installed_for_agent(active_agent_id().as_deref(), Some("astro"))
}

/// `scope`: `"astro"` | `"machine"` | `None`（默认 astro，供 prompt / 兼容）
pub fn list_installed_for_agent(
    agent_id: Option<&str>,
    scope: Option<&str>,
) -> Vec<InstalledSkill> {
    match scope.unwrap_or("astro") {
        "machine" => scan_machine(agent_id),
        "all" => {
            let mut all = scan_astro(agent_id);
            all.extend(scan_machine(agent_id));
            all.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            all
        }
        _ => scan_astro(agent_id),
    }
}

/// 已启用技能 (name, description)，供 Agent system prompt 索引（仅 Astro 范围）
pub fn list_enabled_for_prompt() -> Vec<(String, String)> {
    list_installed()
        .into_iter()
        .filter(|s| s.enabled)
        .map(|s| (s.name, s.description))
        .collect()
}

/// 设置某技能的启用状态（仅 Astro 范围）
pub fn set_enabled(id: &str, enabled: bool) -> Result<()> {
    set_enabled_for_agent(active_agent_id().as_deref(), id, enabled)
}

/// 为指定 Agent（或全局）写入技能启用开关。
pub fn set_enabled_for_agent(agent_id: Option<&str>, id: &str, enabled: bool) -> Result<()> {
    let mut state = load_enabled_state(agent_id);
    state.insert(id.to_string(), enabled);
    save_enabled_state(agent_id, &state)
}

/// 将本机技能软链到当前 Agent 的 `workspace/skills/{name}`
pub fn link_skill_to_agent(agent_id: Option<&str>, id: &str, linked: bool) -> Result<()> {
    let id_norm = normalize_agent_id(agent_id).context("需要指定 Agent")?;
    let skill = scan_machine(Some(&id_norm))
        .into_iter()
        .find(|s| s.id == id)
        .with_context(|| format!("未找到本机技能: {id}"))?;

    let skill_dir = PathBuf::from(&skill.path)
        .parent()
        .map(|p| p.to_path_buf())
        .with_context(|| format!("无效技能路径: {}", skill.path))?;

    let dest_root = agent_workspace(&id_norm).join("skills");
    fs::create_dir_all(&dest_root)
        .with_context(|| format!("create {}", dest_root.display()))?;
    let dest = dest_root.join(&skill.name);

    if linked {
        if dest.exists() {
            if same_path(&dest, &skill_dir) {
                return Ok(());
            }
            bail!(
                "目标已存在且不是该技能的链接: {}",
                dest.display()
            );
        }
        create_skill_link(&skill_dir, &dest)?;
    } else if dest.exists() {
        if same_path(&dest, &skill_dir) {
            fs::remove_file(&dest)
                .or_else(|_| fs::remove_dir_all(&dest))
                .with_context(|| format!("remove {}", dest.display()))?;
        } else {
            bail!("目标不是该技能的链接，未删除: {}", dest.display());
        }
    }
    Ok(())
}

/// 在 Agent skills 目录创建指向源技能的链接/拷贝。
fn create_skill_link(src: &Path, dest: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src, dest)
            .with_context(|| format!("symlink {} → {}", src.display(), dest.display()))?;
        return Ok(());
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(src, dest).or_else(|_| {
            // 无权限建软链时退化为复制目录
            copy_dir_recursive(src, dest)
        })?;
        return Ok(());
    }
    #[cfg(not(any(unix, windows)))]
    {
        copy_dir_recursive(src, dest)
    }
}

#[cfg(any(windows, not(any(unix, windows))))]
/// 递归复制目录树。
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// 按名称加载已启用技能的 SKILL.md 全文
pub fn load_skill_by_name(name: &str) -> Result<LoadedSkill> {
    let installed = list_installed()
        .into_iter()
        .find(|s| s.name == name)
        .with_context(|| format!("未找到技能: {name}"))?;

    if !installed.enabled {
        anyhow::bail!("技能已禁用: {name}");
    }

    let content = fs::read_to_string(&installed.path)
        .with_context(|| format!("读取 {}", installed.path))?;

    Ok(LoadedSkill {
        metadata: SkillMetadata {
            name: installed.name,
            description: installed.description,
        },
        path: std::path::PathBuf::from(&installed.path),
        content,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn scan_and_load_skill_by_name() {
        let _guard = ENV_TEST_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("skills/demo-skill-xyz");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo-skill-xyz\ndescription: test desc\n---\n# Hello\n",
        )
        .unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        std::env::remove_var("ASTRO_WORKSPACE");

        let list = list_installed_for_agent(Some("workspace"), Some("astro"));
        assert!(list.iter().any(|s| s.name == "demo-skill-xyz"));

        // active agent for load_skill_by_name
        fs::write(
            dir.path().join("active-agent.json"),
            r#"{"id":"workspace"}"#,
        )
        .unwrap();
        let loaded = load_skill_by_name("demo-skill-xyz").unwrap();
        assert!(loaded.content.contains("# Hello"));
        assert_eq!(loaded.metadata.description, "test desc");
    }

    #[test]
    fn load_skill_by_name_rejects_disabled() {
        let _guard = ENV_TEST_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("skills/off-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: off-skill\ndescription: off\n---\n",
        )
        .unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        std::env::remove_var("ASTRO_WORKSPACE");
        fs::write(
            dir.path().join("active-agent.json"),
            r#"{"id":"workspace"}"#,
        )
        .unwrap();

        let installed = list_installed_for_agent(Some("workspace"), Some("astro"))
            .into_iter()
            .find(|s| s.name == "off-skill")
            .expect("off-skill");
        set_enabled_for_agent(Some("workspace"), &installed.id, false).unwrap();

        let err = load_skill_by_name("off-skill").unwrap_err();
        assert!(err.to_string().contains("禁用"));
    }

    #[test]
    fn machine_scope_excludes_astro_skills() {
        let _guard = ENV_TEST_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let astro_skill = dir.path().join("skills/astro-only");
        fs::create_dir_all(&astro_skill).unwrap();
        fs::write(
            astro_skill.join("SKILL.md"),
            "---\nname: astro-only\ndescription: a\n---\n",
        )
        .unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let astro = list_installed_for_agent(Some("workspace"), Some("astro"));
        assert!(astro.iter().any(|s| s.name == "astro-only"));
        let machine = list_installed_for_agent(Some("workspace"), Some("machine"));
        assert!(!machine.iter().any(|s| s.name == "astro-only"));
    }
}
