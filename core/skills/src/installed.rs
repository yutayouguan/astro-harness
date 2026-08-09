//! 本机 Skill 扫描、启用状态与按名称加载。

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
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
    agent_id.map(str::trim).filter(|s| !s.is_empty()).map(|s| {
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
        .and_then(|v| v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
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
    } else if let Some(ws) = crate::workspace_override() {
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
    let meta = parse_skill_frontmatter_full(content);
    (meta.name, meta.description)
}

/// 解析 frontmatter（含可选 `astro_tools` 列表）。
pub fn parse_skill_frontmatter_full(content: &str) -> SkillMetadata {
    let mut name = String::new();
    let mut description = String::new();
    let mut astro_tools = Vec::new();
    if let Some(rest) = content.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let block = &rest[..end];
            let mut in_astro_tools = false;
            for line in block.lines() {
                let trimmed = line.trim();
                if let Some(v) = trimmed.strip_prefix("name:") {
                    name = v.trim().trim_matches('"').to_string();
                    in_astro_tools = false;
                } else if let Some(v) = trimmed.strip_prefix("description:") {
                    description = v.trim().trim_matches('"').to_string();
                    in_astro_tools = false;
                } else if let Some(rest) = trimmed.strip_prefix("astro_tools:") {
                    in_astro_tools = true;
                    let rest = rest.trim();
                    if rest.starts_with('[') {
                        // inline: astro_tools: [a, b]
                        for part in rest.trim_matches(|c| c == '[' || c == ']').split(',') {
                            let t = part.trim().trim_matches('"').trim_matches('\'').to_string();
                            if !t.is_empty() {
                                astro_tools.push(t);
                            }
                        }
                        in_astro_tools = false;
                    }
                } else if in_astro_tools {
                    if let Some(item) = trimmed.strip_prefix("- ") {
                        let t = item.trim().trim_matches('"').trim_matches('\'').to_string();
                        if !t.is_empty() {
                            astro_tools.push(t);
                        }
                    } else if !trimmed.is_empty() && !trimmed.starts_with('-') {
                        in_astro_tools = false;
                    }
                }
            }
        }
    }
    SkillMetadata {
        name,
        description,
        astro_tools,
    }
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

    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// 扫描本机其它技能根（如 Claude 共享目录）。
fn scan_machine(agent_id: Option<&str>) -> Vec<InstalledSkill> {
    let roots = machine_skill_roots();
    let mut state = HashMap::new();
    let mut dirty = false;
    let mut out = scan_roots(&roots, agent_id, "machine", &mut state, &mut dirty, false);
    out.sort_by_key(|a| a.name.to_lowercase());
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
            all.sort_by_key(|a| a.name.to_lowercase());
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
    fs::create_dir_all(&dest_root).with_context(|| format!("create {}", dest_root.display()))?;
    let dest = dest_root.join(&skill.name);

    if linked {
        if dest.exists() {
            if same_path(&dest, &skill_dir) {
                return Ok(());
            }
            bail!("目标已存在且不是该技能的链接: {}", dest.display());
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
        Ok(())
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

/// 最近一次成功加载的技能记忆（仅供同一次 skills 工具调用内的 additive toolset
/// 激活复用，避免紧接着再次扫描 + 读盘）。窗口极短，不影响 enable/disable 正确性。
struct RecentLoad {
    name: String,
    astro_tools: Vec<String>,
    at: Instant,
}

static RECENT_LOAD: Mutex<Option<RecentLoad>> = Mutex::new(None);

/// 最近一次成功加载 `name` 的 `astro_tools`（若在 [`RECENT_LOAD_TTL`] 内）。
///
/// 用于 skills 工具执行后立即做 toolset 激活时复用，省去二次磁盘加载。
pub fn recent_astro_tools(name: &str) -> Option<Vec<String>> {
    const RECENT_LOAD_TTL: Duration = Duration::from_secs(5);
    let guard = RECENT_LOAD.lock().ok()?;
    let recent = guard.as_ref()?;
    if recent.name == name && recent.at.elapsed() < RECENT_LOAD_TTL {
        Some(recent.astro_tools.clone())
    } else {
        None
    }
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

    let content =
        fs::read_to_string(&installed.path).with_context(|| format!("读取 {}", installed.path))?;

    let loaded = LoadedSkill {
        metadata: {
            let mut meta = parse_skill_frontmatter_full(&content);
            if meta.name.is_empty() {
                meta.name = installed.name.clone();
            }
            if meta.description.is_empty() {
                meta.description = installed.description.clone();
            }
            meta
        },
        path: std::path::PathBuf::from(&installed.path),
        content,
    };

    if let Ok(mut guard) = RECENT_LOAD.lock() {
        *guard = Some(RecentLoad {
            name: name.to_string(),
            astro_tools: loaded.metadata.astro_tools.clone(),
            at: Instant::now(),
        });
    }
    crate::usage::record_skill_load(name);

    Ok(loaded)
}

const MAX_SKILL_FILE_PREVIEW_BYTES: u64 = 512 * 1024;

/// 预览用：可按 id 精确定位（本机/Astro），否则按名称在全部范围查找。
fn find_installed_for_preview(name: &str, id: Option<&str>) -> Result<InstalledSkill> {
    let agent = active_agent_id();
    let all = list_installed_for_agent(agent.as_deref(), Some("all"));
    if let Some(id) = id.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(skill) = all.iter().find(|s| s.id == id) {
            return Ok(skill.clone());
        }
    }
    all.into_iter()
        .find(|s| s.name == name)
        .with_context(|| format!("未找到技能: {name}"))
}

fn skill_root_of(installed: &InstalledSkill) -> Result<PathBuf> {
    let skill_md = PathBuf::from(&installed.path);
    skill_md
        .parent()
        .map(PathBuf::from)
        .with_context(|| format!("无效技能路径: {}", installed.path))
}

fn skill_file_category(rel: &str) -> &'static str {
    let lower = rel.to_ascii_lowercase();
    if lower == "skill.md" {
        return "overview";
    }
    if lower.starts_with("scripts/") || lower == "scripts" {
        return "scripts";
    }
    if lower.starts_with("references/")
        || lower.starts_with("reference/")
        || lower == "references"
        || lower == "reference"
    {
        return "references";
    }
    if lower.starts_with("assets/")
        || lower.starts_with("asset/")
        || lower == "assets"
        || lower == "asset"
    {
        return "assets";
    }
    "other"
}

fn is_probably_text_file(path: &Path) -> bool {
    const TEXT_EXT: &[&str] = &[
        "md",
        "txt",
        "json",
        "yaml",
        "yml",
        "toml",
        "xml",
        "html",
        "htm",
        "css",
        "scss",
        "js",
        "jsx",
        "ts",
        "tsx",
        "mjs",
        "cjs",
        "py",
        "rb",
        "go",
        "rs",
        "java",
        "kt",
        "swift",
        "c",
        "cc",
        "cpp",
        "h",
        "hpp",
        "cs",
        "php",
        "sh",
        "bash",
        "zsh",
        "fish",
        "ps1",
        "bat",
        "cmd",
        "sql",
        "graphql",
        "vue",
        "svelte",
        "astro",
        "ini",
        "cfg",
        "conf",
        "env",
        "gitignore",
        "dockerignore",
        "editorconfig",
        "csv",
        "tsv",
        "log",
        "r",
        "lua",
        "pl",
        "pm",
        "scala",
        "dart",
        "zig",
        "nim",
        "ex",
        "exs",
        "erl",
        "hs",
        "clj",
        "lisp",
        "el",
        "makefile",
        "dockerfile",
        "cmake",
        "gradle",
        "properties",
        "plist",
    ];
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if name == "skill.md"
        || name == "makefile"
        || name == "dockerfile"
        || name == "license"
        || name == "licence"
        || name == "readme"
        || name == "changelog"
    {
        return true;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let ext = e.to_ascii_lowercase();
            TEXT_EXT.iter().any(|t| *t == ext)
        })
        .unwrap_or(false)
}

fn walk_skill_files(root: &Path, dir: &Path, out: &mut Vec<crate::models::SkillFileEntry>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk_skill_files(root, &path, out);
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let relative_path = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if relative_path.is_empty() {
            continue;
        }
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        out.push(crate::models::SkillFileEntry {
            category: skill_file_category(&relative_path).to_string(),
            is_text: is_probably_text_file(&path),
            size,
            relative_path,
        });
    }
}

fn category_rank(cat: &str) -> u8 {
    match cat {
        "overview" => 0,
        "scripts" => 1,
        "references" => 2,
        "assets" => 3,
        _ => 4,
    }
}

/// 列出技能目录内全部文件（含禁用/本机技能，供 UI 预览）。
pub fn list_skill_files(name: &str) -> Result<crate::models::SkillBundle> {
    list_skill_files_ex(name, None)
}

/// 同上，可选 skill id（本机与 Astro 重名时用）。
pub fn list_skill_files_ex(name: &str, id: Option<&str>) -> Result<crate::models::SkillBundle> {
    let installed = find_installed_for_preview(name, id)?;
    let root = skill_root_of(&installed)?;
    let mut files = Vec::new();
    walk_skill_files(&root, &root, &mut files);
    files.sort_by(|a, b| {
        category_rank(&a.category)
            .cmp(&category_rank(&b.category))
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    Ok(crate::models::SkillBundle {
        name: installed.name,
        description: installed.description,
        root: root.to_string_lossy().to_string(),
        files,
    })
}

/// 读取技能目录内某个相对路径的文本内容（防穿越）。
pub fn read_skill_file(name: &str, relative_path: &str) -> Result<String> {
    read_skill_file_ex(name, relative_path, None)
}

/// 同上，可选 skill id。
pub fn read_skill_file_ex(name: &str, relative_path: &str, id: Option<&str>) -> Result<String> {
    let installed = find_installed_for_preview(name, id)?;
    let root = skill_root_of(&installed)?.canonicalize()?;
    let rel = relative_path.trim().trim_start_matches('/');
    if rel.is_empty() || rel.contains("..") {
        anyhow::bail!("非法路径");
    }
    let joined = root.join(rel);
    let canon = joined
        .canonicalize()
        .with_context(|| format!("文件不存在: {rel}"))?;
    if !canon.starts_with(&root) {
        anyhow::bail!("路径越界");
    }
    if !canon.is_file() {
        anyhow::bail!("不是文件: {rel}");
    }
    let meta = fs::metadata(&canon)?;
    if meta.len() > MAX_SKILL_FILE_PREVIEW_BYTES {
        anyhow::bail!(
            "文件过大（{} > {} 字节），请在外部打开",
            meta.len(),
            MAX_SKILL_FILE_PREVIEW_BYTES
        );
    }
    if !is_probably_text_file(&canon) {
        anyhow::bail!("二进制或不支持预览的文件类型");
    }
    fs::read_to_string(&canon).with_context(|| format!("读取失败: {rel}"))
}

/// 解析技能内绝对路径（防穿越），供打开/定位使用。
fn resolve_skill_abs_path(
    name: &str,
    relative_path: Option<&str>,
    id: Option<&str>,
) -> Result<PathBuf> {
    let installed = find_installed_for_preview(name, id)?;
    let root = skill_root_of(&installed)?.canonicalize()?;
    let Some(rel_raw) = relative_path.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(root);
    };
    let rel = rel_raw.trim_start_matches('/');
    if rel.contains("..") {
        anyhow::bail!("非法路径");
    }
    let joined = root.join(rel);
    let canon = joined
        .canonicalize()
        .with_context(|| format!("路径不存在: {rel}"))?;
    if !canon.starts_with(&root) {
        anyhow::bail!("路径越界");
    }
    Ok(canon)
}

fn open_path_with_system(path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .with_context(|| format!("无法打开 {}", path.display()))?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .spawn()
            .with_context(|| format!("无法打开 {}", path.display()))?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .with_context(|| format!("无法打开 {}", path.display()))?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        let _ = path;
        anyhow::bail!("当前平台不支持用系统应用打开")
    }
}

/// 在系统文件管理器中定位路径（供 `backups` 等模块复用）。
pub(crate) fn reveal_path_in_file_manager(path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R"])
            .arg(path)
            .spawn()
            .with_context(|| format!("无法定位 {}", path.display()))?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.to_string_lossy()))
            .spawn()
            .with_context(|| format!("无法定位 {}", path.display()))?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let parent = path.parent().unwrap_or(path);
        open_path_with_system(parent)?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        let _ = path;
        anyhow::bail!("当前平台不支持在文件管理器中显示")
    }
}

/// 在系统文件管理器中打开技能根目录。
pub fn open_skill_folder(name: &str, id: Option<&str>) -> Result<()> {
    let root = resolve_skill_abs_path(name, None, id)?;
    open_path_with_system(&root)
}

/// 在文件管理器中选中/显示技能内某个文件（或根目录）。
pub fn reveal_skill_file(name: &str, relative_path: &str, id: Option<&str>) -> Result<()> {
    let path = resolve_skill_abs_path(name, Some(relative_path), id)?;
    reveal_path_in_file_manager(&path)
}

/// 用系统默认应用打开技能内某个文件。
pub fn open_skill_file_externally(name: &str, relative_path: &str, id: Option<&str>) -> Result<()> {
    let path = resolve_skill_abs_path(name, Some(relative_path), id)?;
    open_path_with_system(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    static ENV_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[test]
    fn scan_and_load_skill_by_name() {
        let _guard = ENV_TEST_LOCK.blocking_lock();
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
        let _guard = ENV_TEST_LOCK.blocking_lock();
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
    fn list_skill_files_groups_categories() {
        let _guard = ENV_TEST_LOCK.blocking_lock();
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("skills/bundle-skill");
        fs::create_dir_all(skill_dir.join("scripts")).unwrap();
        fs::create_dir_all(skill_dir.join("references")).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: bundle-skill\ndescription: bundled\n---\n# Body\n",
        )
        .unwrap();
        fs::write(skill_dir.join("scripts/run.py"), "print(1)\n").unwrap();
        fs::write(skill_dir.join("references/notes.md"), "# notes\n").unwrap();
        fs::write(skill_dir.join("logo.png"), [0u8, 1, 2, 3]).unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        std::env::remove_var("ASTRO_WORKSPACE");
        fs::write(
            dir.path().join("active-agent.json"),
            r#"{"id":"workspace"}"#,
        )
        .unwrap();

        let bundle = list_skill_files("bundle-skill").unwrap();
        assert_eq!(bundle.name, "bundle-skill");
        assert!(bundle
            .files
            .iter()
            .any(|f| f.relative_path == "SKILL.md" && f.category == "overview" && f.is_text));
        assert!(bundle
            .files
            .iter()
            .any(|f| f.relative_path == "scripts/run.py" && f.category == "scripts"));
        assert!(bundle
            .files
            .iter()
            .any(|f| f.relative_path == "references/notes.md" && f.category == "references"));
        assert!(bundle
            .files
            .iter()
            .any(|f| f.relative_path == "logo.png" && f.category == "other" && !f.is_text));

        let md = read_skill_file("bundle-skill", "SKILL.md").unwrap();
        assert!(md.contains("# Body"));
        let py = read_skill_file("bundle-skill", "scripts/run.py").unwrap();
        assert!(py.contains("print"));
        assert!(read_skill_file("bundle-skill", "logo.png").is_err());
        assert!(read_skill_file("bundle-skill", "../outside.md").is_err());
    }

    #[test]
    fn machine_scope_excludes_astro_skills() {
        let _guard = ENV_TEST_LOCK.blocking_lock();
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

    #[test]
    fn parse_astro_tools_list_and_inline() {
        let block =
            "---\nname: t\ndescription: d\nastro_tools:\n  - terminal\n  - file_ops\n---\nbody\n";
        let m = parse_skill_frontmatter_full(block);
        assert_eq!(m.name, "t");
        assert_eq!(m.astro_tools, vec!["terminal", "file_ops"]);

        let inline = "---\nname: t2\ndescription: d\nastro_tools: [web_search, browser]\n---\n";
        let m2 = parse_skill_frontmatter_full(inline);
        assert_eq!(m2.astro_tools, vec!["web_search", "browser"]);
    }

    #[test]
    fn recent_astro_tools_reused_after_load() {
        let _guard = ENV_TEST_LOCK.blocking_lock();
        let dir = tempdir().unwrap();
        let skill_dir = dir.path().join("skills/recent-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: recent-skill\ndescription: d\nastro_tools: [terminal, web_search]\n---\nbody\n",
        )
        .unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        std::env::remove_var("ASTRO_WORKSPACE");
        fs::write(
            dir.path().join("active-agent.json"),
            r#"{"id":"workspace"}"#,
        )
        .unwrap();

        assert!(recent_astro_tools("recent-skill").is_none());
        let loaded = load_skill_by_name("recent-skill").unwrap();
        assert_eq!(loaded.metadata.astro_tools, vec!["terminal", "web_search"]);
        assert_eq!(
            recent_astro_tools("recent-skill"),
            Some(vec!["terminal".to_string(), "web_search".to_string()])
        );
        assert!(recent_astro_tools("other-skill").is_none());
    }
}
