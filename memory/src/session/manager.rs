//! 记忆管理器：聚合 Markdown 记忆，供 Agent Prompt 与 `memory` 工具调用。
//!
//! [`MemoryManager`] 绑定单个 Agent 工作区，统一管理 `MEMORY.md`、`USER.md`、每日记忆。
//! 会话读写与 `session_search` 由 `session` crate 提供。

use std::path::PathBuf;

use crate::config::{load_memory_config, MemoryConfig};
use crate::workspace::ensure_workspace;
use crate::MemoryStore;
use home::{
    active_agent_id, daily_memory_path, ensure_agent_space, ensure_daily_memory,
    normalize_agent_id, today_date_string, DEFAULT_AGENT_ID,
};

/// 记忆写入/替换/删除的目标存储位置（工具面仅 MEMORY / USER）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryTarget {
    /// `MEMORY.md` — 长期精炼记忆。
    Memory,
    /// `USER.md` — 用户档案。
    User,
}

/// 单 Agent 记忆子系统的聚合入口。
///
/// 构造时会确保工作区与 Agent 空间存在；会话库由 [`ensure_workspace`] 初始化。
pub struct MemoryManager {
    /// Astro 数据根目录（通常为 `~/.astro`）。
    pub base_dir: PathBuf,
    /// 当前绑定的 Agent 标识（已规范化）。
    pub agent_id: String,
    /// Agent 工作区目录，含 `MEMORY.md`、`USER.md`、`memory/` 等。
    pub workspace_dir: PathBuf,
    /// 长期记忆存储（live + snapshot）。
    pub memory: MemoryStore,
    /// 用户档案存储（live + snapshot）。
    pub user: MemoryStore,
    /// 从 `{base_dir}/config.yaml` 加载的记忆配置。
    pub config: MemoryConfig,
}

impl MemoryManager {
    /// 以 `base_dir` 下当前活跃 Agent 构造管理器。
    ///
    /// 活跃 Agent 由 `active_agent_id` 决定；空或缺失时回退默认 Agent。
    pub fn new(base_dir: PathBuf) -> anyhow::Result<Self> {
        ensure_workspace(&base_dir)?;
        let agent_id = active_agent_id(&base_dir);
        Self::for_agent(base_dir, &agent_id)
    }

    /// 为指定 `agent_id` 构造管理器；空白 id 回退 [`DEFAULT_AGENT_ID`]。
    ///
    /// 会创建 Agent 工作区（若不存在）；会话库由 [`ensure_workspace`] 初始化。
    pub fn for_agent(base_dir: PathBuf, agent_id: &str) -> anyhow::Result<Self> {
        ensure_workspace(&base_dir)?;
        let id = if agent_id.trim().is_empty() {
            DEFAULT_AGENT_ID.to_string()
        } else {
            normalize_agent_id(agent_id)
        };
        let workspace = ensure_agent_space(&base_dir, &id, None)?;
        let config = load_memory_config(&base_dir);
        let memory = MemoryStore::open(workspace.join("MEMORY.md"), config.memory_char_limit)?;
        let user = MemoryStore::open(workspace.join("USER.md"), config.user_char_limit)?;
        Ok(MemoryManager {
            base_dir,
            agent_id: id,
            workspace_dir: workspace,
            memory,
            user,
            config,
        })
    }

    /// 今日每日记忆文件路径（必要时创建）
    pub fn today_daily_path(&self) -> anyhow::Result<PathBuf> {
        let date = today_date_string();
        ensure_daily_memory(&self.workspace_dir, &date)
    }

    /// 读取今日每日记忆全文（供 Prompt 注入）
    pub fn daily_content_today(&self) -> String {
        let date = today_date_string();
        let path = daily_memory_path(&self.workspace_dir, &date);
        std::fs::read_to_string(path).unwrap_or_default()
    }

    /// 向每日记忆追加一段（按日期，默认今天）。非工具入口；日记请走系统/入梦管线。
    pub fn append_daily(&self, entry: &str, date: Option<&str>) -> anyhow::Result<String> {
        let date = date
            .map(|s| s.to_string())
            .unwrap_or_else(today_date_string);
        let path = ensure_daily_memory(&self.workspace_dir, &date)?;
        let mut content = std::fs::read_to_string(&path).unwrap_or_default();
        if !content.ends_with('\n') && !content.is_empty() {
            content.push('\n');
        }
        let line = entry.trim();
        if line.starts_with('-') || line.starts_with('*') {
            content.push_str(line);
        } else {
            content.push_str("- ");
            content.push_str(line);
        }
        content.push('\n');
        std::fs::write(&path, &content)?;
        Ok(format!("已写入每日记忆 memory/{date}.md"))
    }

    /// 返回 `(长期记忆, 用户档案)` 的 **snapshot** 渲染文本，供 Prompt 注入。
    ///
    /// 与 [`prompt_snapshot_with_daily`] 相同语义（不含日记）；受 `memory_*_enabled` 开关影响。
    pub fn prompt_content(&self) -> (String, String) {
        let (mem, user, _) = self.prompt_snapshot_with_daily();
        (mem, user)
    }

    /// Prompt 注入用：MEMORY / USER 取 **snapshot**；今日日记读盘后截断到配置上限。
    pub fn prompt_snapshot_with_daily(&self) -> (String, String, String) {
        let mem = if self.config.memory_enabled {
            self.memory.snapshot_render()
        } else {
            String::new()
        };
        let user = if self.config.user_profile_enabled {
            self.user.snapshot_render()
        } else {
            String::new()
        };
        let daily = truncate_chars(
            &self.daily_content_today(),
            self.config.daily_prompt_max_chars,
        );
        (mem, user, daily)
    }

    /// 从磁盘重载 MEMORY / USER 到 live，并同步 snapshot（供换 session / 显式 refresh）。
    ///
    /// 同时重读 `config.yaml` 的 `memory:` 段，使 `write_approval` 等开关即时生效。
    pub fn refresh_memory_snapshot(&mut self) -> anyhow::Result<()> {
        self.config = load_memory_config(&self.base_dir);
        self.memory.reload()?;
        self.user.reload()?;
        Ok(())
    }

    /// 从磁盘重读记忆配置（开关变更后、不换 snapshot 时可用）。
    pub fn reload_memory_config(&mut self) {
        self.config = load_memory_config(&self.base_dir);
    }

    /// 统一处理 `memory` 工具的 action / target。
    ///
    /// 当 [`MemoryConfig::write_approval`] 为 true 时，将变更入 pending 队列而不改 live。
    pub fn handle_memory_op(
        &mut self,
        action: &str,
        target: MemoryTarget,
        content: Option<&str>,
        old_text: Option<&str>,
    ) -> anyhow::Result<String> {
        self.handle_memory_op_with_source(action, target, content, old_text, "tool")
    }

    /// 同 [`handle_memory_op`]，可指定 pending 的 `source`（`tool` / `review`）。
    pub fn handle_memory_op_with_source(
        &mut self,
        action: &str,
        target: MemoryTarget,
        content: Option<&str>,
        old_text: Option<&str>,
        source: &str,
    ) -> anyhow::Result<String> {
        self.reload_memory_config();
        if self.config.write_approval {
            return self.enqueue_memory_op(action, target, content, old_text, source);
        }
        self.apply_memory_op_direct(action, target, content, old_text)
    }

    /// 直接写 live（供 approve / `write_approval=false` 使用；不经 pending）。
    pub fn apply_memory_op_direct(
        &mut self,
        action: &str,
        target: MemoryTarget,
        content: Option<&str>,
        old_text: Option<&str>,
    ) -> anyhow::Result<String> {
        match action {
            "add" => {
                let content = content
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("缺少 content 参数"))?;
                self.handle_memory_add(content, target)
            }
            "replace" => {
                let old_text = old_text.ok_or_else(|| anyhow::anyhow!("缺少 old_text 参数"))?;
                let content = content.ok_or_else(|| anyhow::anyhow!("缺少 content 参数"))?;
                self.handle_memory_replace(old_text, content, target)
            }
            "remove" => {
                let old_text = old_text.ok_or_else(|| anyhow::anyhow!("缺少 old_text 参数"))?;
                self.handle_memory_remove(old_text, target)
            }
            other => anyhow::bail!("未知 memory action: {other}（期望 add|replace|remove）"),
        }
    }

    fn enqueue_memory_op(
        &self,
        action: &str,
        target: MemoryTarget,
        content: Option<&str>,
        old_text: Option<&str>,
        source: &str,
    ) -> anyhow::Result<String> {
        // 参数校验与直接写入路径对齐（扫描在 pending::enqueue 内完成）
        match action {
            "add" => {
                let _ = content
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("缺少 content 参数"))?;
            }
            "replace" => {
                let _ = old_text.ok_or_else(|| anyhow::anyhow!("缺少 old_text 参数"))?;
                let _ = content.ok_or_else(|| anyhow::anyhow!("缺少 content 参数"))?;
            }
            "remove" => {
                let _ = old_text.ok_or_else(|| anyhow::anyhow!("缺少 old_text 参数"))?;
            }
            other => anyhow::bail!("未知 memory action: {other}（期望 add|replace|remove）"),
        }

        let pending = crate::pending::enqueue(
            &self.base_dir,
            crate::pending::PendingMemoryWrite {
                id: String::new(),
                agent_id: self.agent_id.clone(),
                target,
                action: action.to_string(),
                content: content.map(|s| s.to_string()),
                old_text: old_text.map(|s| s.to_string()),
                source: source.to_string(),
                created_at: String::new(),
            },
        )?;

        Ok(format!(
            "写入已入队待审批（id={}，action={}，target={:?}）；未改动 live",
            pending.id, pending.action, pending.target
        ))
    }

    /// 向指定目标追加一条记忆；返回面向用户的中文操作结果（含用量）。
    pub fn handle_memory_add(
        &mut self,
        entry: &str,
        target: MemoryTarget,
    ) -> anyhow::Result<String> {
        match target {
            MemoryTarget::Memory => {
                let result = self.memory.add(entry)?;
                Ok(format_write_message("长期记忆 MEMORY.md", &result))
            }
            MemoryTarget::User => {
                let result = self.user.add(entry)?;
                Ok(format_write_message("用户档案 USER.md", &result))
            }
        }
    }

    /// 在指定目标中替换唯一匹配 `old_text` 的条目。
    pub fn handle_memory_replace(
        &mut self,
        old_text: &str,
        new_text: &str,
        target: MemoryTarget,
    ) -> anyhow::Result<String> {
        match target {
            MemoryTarget::Memory => {
                let result = self.memory.replace(old_text, new_text)?;
                Ok(format_op_message(&result))
            }
            MemoryTarget::User => {
                let result = self.user.replace(old_text, new_text)?;
                Ok(format_op_message(&result))
            }
        }
    }

    /// 从指定目标删除唯一包含 `text` 的条目。
    pub fn handle_memory_remove(
        &mut self,
        text: &str,
        target: MemoryTarget,
    ) -> anyhow::Result<String> {
        match target {
            MemoryTarget::Memory => {
                let result = self.memory.remove(text)?;
                Ok(format_op_message(&result))
            }
            MemoryTarget::User => {
                let result = self.user.remove(text)?;
                Ok(format_op_message(&result))
            }
        }
    }
}

const SNAPSHOT_NOTE: &str = "已写盘（live）；当前会话 prompt 快照未刷新";

fn format_write_message(label: &str, result: &crate::MemoryWriteResult) -> String {
    if result.duplicate {
        format!(
            "记忆条目已存在于{label}（用量 {}）；{SNAPSHOT_NOTE}",
            result.usage
        )
    } else {
        format!("已写入{label}（用量 {}）；{SNAPSHOT_NOTE}", result.usage)
    }
}

fn format_op_message(result: &crate::MemoryWriteResult) -> String {
    format!(
        "{}（用量 {}）；{SNAPSHOT_NOTE}",
        result.message, result.usage
    )
}

/// 按字符边界截断到 `max` 个字符。
fn truncate_chars(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

/// 将工具参数 `target` 字符串解析为 [`MemoryTarget`]。
///
/// - `"user"` → [`MemoryTarget::User`]
/// - `"memory"` / 缺省 → [`MemoryTarget::Memory`]
/// - `"daily"` / `"mermaid"`（遗留目录名）→ 错误（日记仅系统入口）
fn parse_memory_target(value: Option<&str>) -> anyhow::Result<MemoryTarget> {
    match value {
        None => Ok(MemoryTarget::Memory),
        Some("user") => Ok(MemoryTarget::User),
        Some("memory") => Ok(MemoryTarget::Memory),
        Some("daily") | Some("mermaid") => {
            anyhow::bail!("日记请使用系统入口，不支持 memory 工具 target=daily")
        }
        Some(other) => anyhow::bail!("未知的 memory target: {other}"),
    }
}

/// 记忆相关 Agent 工具的统一分发入口。
///
/// 仅支持工具名 `memory`。参数从 `args` JSON 提取，`target` 经 [`parse_memory_target`] 解析。
pub fn dispatch_memory_tool(
    memory: &mut MemoryManager,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    match name {
        "memory" => {
            let action = args["action"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("缺少 action 参数"))?;
            let target = parse_memory_target(args["target"].as_str())?;
            let content = args["content"].as_str();
            let old_text = args["old_text"].as_str();
            memory.handle_memory_op(action, target, content, old_text)
        }
        _ => anyhow::bail!("未知记忆工具: {name}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn for_agent_opens_stores_with_config_limits() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            "memory:\n  memory_char_limit: 80\n  user_char_limit: 60\n",
        )
        .unwrap();
        let mut mgr = MemoryManager::for_agent(dir.path().to_path_buf(), "main").unwrap();
        assert_eq!(mgr.config.memory_char_limit, 80);
        assert_eq!(mgr.config.user_char_limit, 60);

        // 超限应报错（验证 store 使用了配置上限；模板可能已占用部分配额）
        let long = "x".repeat(100);
        let err = mgr.memory.add(&long).unwrap_err().to_string();
        assert!(
            err.contains("上限") || err.contains("limit") || err.contains("字符"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn prompt_snapshot_respects_enabled_flags_and_truncates_daily() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            r#"
memory:
  memory_enabled: false
  user_profile_enabled: true
  daily_prompt_max_chars: 10
"#,
        )
        .unwrap();
        let mut mgr = MemoryManager::for_agent(dir.path().to_path_buf(), "main").unwrap();
        mgr.memory.add("alpha-entry").unwrap();
        mgr.user.add("prefer-dark-mode").unwrap();
        // live 写入不会自动进 snapshot；reload 后 snapshot 跟上
        mgr.refresh_memory_snapshot().unwrap();

        mgr.append_daily("0123456789ABCDEF", None).unwrap();

        let full_daily = mgr.daily_content_today();
        let (mem, user, daily) = mgr.prompt_snapshot_with_daily();
        assert!(mem.is_empty(), "memory_enabled=false → empty mem");
        assert!(
            user.contains("prefer-dark-mode"),
            "user snapshot must contain added entry; got: {user:?}"
        );
        assert!(
            full_daily.starts_with(&format!("# {}", today_date_string())),
            "daily file must use today's date header"
        );
        assert_eq!(daily.chars().count(), 10);
        assert_eq!(
            daily,
            full_daily.chars().take(10).collect::<String>(),
            "daily must be exact char-boundary prefix of full content"
        );
    }

    #[test]
    fn parse_daily_target_errors() {
        let err = parse_memory_target(Some("daily")).unwrap_err().to_string();
        assert!(err.contains("日记"));
        assert!(matches!(
            parse_memory_target(Some("memory")).unwrap(),
            MemoryTarget::Memory
        ));
        assert!(matches!(
            parse_memory_target(Some("user")).unwrap(),
            MemoryTarget::User
        ));
        assert!(matches!(
            parse_memory_target(None).unwrap(),
            MemoryTarget::Memory
        ));
        let err = parse_memory_target(Some("project"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("未知的 memory target"));
        let err = parse_memory_target(Some("bogus")).unwrap_err().to_string();
        assert!(err.contains("未知的 memory target"));
    }

    #[test]
    fn dispatch_memory_add_replace_remove() {
        let dir = tempfile::tempdir().unwrap();
        let mut mgr = MemoryManager::for_agent(dir.path().to_path_buf(), "main").unwrap();

        let added = dispatch_memory_tool(
            &mut mgr,
            "memory",
            &serde_json::json!({
                "action": "add",
                "target": "memory",
                "content": "喜欢深色主题"
            }),
        )
        .unwrap();
        assert!(added.contains("已写盘（live）"));
        assert!(added.contains("快照未刷新"));
        assert!(mgr
            .memory
            .live_entries()
            .iter()
            .any(|e| e.contains("深色主题")));

        let replaced = dispatch_memory_tool(
            &mut mgr,
            "memory",
            &serde_json::json!({
                "action": "replace",
                "target": "memory",
                "old_text": "深色主题",
                "content": "喜欢浅色主题"
            }),
        )
        .unwrap();
        assert!(
            replaced.contains("已替换")
                || replaced.contains("浅色主题")
                || replaced.contains("已写盘")
        );
        assert!(mgr
            .memory
            .live_entries()
            .iter()
            .any(|e| e.contains("浅色主题")));

        let removed = dispatch_memory_tool(
            &mut mgr,
            "memory",
            &serde_json::json!({
                "action": "remove",
                "target": "memory",
                "old_text": "浅色主题"
            }),
        )
        .unwrap();
        assert!(removed.contains("已删除") || removed.contains("已写盘"));
        assert!(!mgr
            .memory
            .live_entries()
            .iter()
            .any(|e| e.contains("浅色主题")));
    }

    #[test]
    fn dispatch_rejects_unknown_tool_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut mgr = MemoryManager::for_agent(dir.path().to_path_buf(), "main").unwrap();
        for name in ["memory_add", "memory_replace", "memory_remove"] {
            let err = dispatch_memory_tool(&mut mgr, name, &serde_json::json!({}))
                .unwrap_err()
                .to_string();
            assert!(err.contains("未知记忆工具"), "unexpected for {name}: {err}");
        }
    }
}
