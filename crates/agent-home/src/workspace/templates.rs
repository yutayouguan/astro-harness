//! 工作区 Markdown 模板与根目录状态文件种子。

/// 工作区核心 Markdown 模板（文件名 → 模板正文，含 `{{ID}}` / `{{NAME}}` 占位符）
pub(crate) const CORE_FILES: &[(&str, &str)] = &[
    ("IDENTITY.md", TEMPLATE_IDENTITY),
    ("USER.md", TEMPLATE_USER),
    ("SOUL.md", TEMPLATE_SOUL),
    ("AGENTS.md", TEMPLATE_AGENTS),
    ("TOOLS.md", TEMPLATE_TOOLS),
    ("MEMORY.md", TEMPLATE_MEMORY),
];

/// Agent 工作区内需要确保存在的子目录
pub(crate) const AGENT_SUBDIRS: &[&str] = &["memory", "skills"];

pub(crate) const TEMPLATE_IDENTITY: &str = include_str!("templates/IDENTITY.md");

pub(crate) const TEMPLATE_USER: &str = include_str!("templates/USER.md");

pub(crate) const TEMPLATE_SOUL: &str = include_str!("templates/SOUL.md");

pub(crate) const TEMPLATE_AGENTS: &str = include_str!("templates/AGENTS.md");

pub(crate) const TEMPLATE_TOOLS: &str = include_str!("templates/TOOLS.md");

pub(crate) const TEMPLATE_MEMORY: &str = r#"# MEMORY.md — 长期精炼记忆

跨会话保留的结构化事实。只写提炼后的结论，日常流水请写入 `memory/YYYY-MM-DD.md`。

- Astro 记忆空间已初始化
"#;

/// 数据根下需要确保存在的目录（相对 `~/.astro`）
pub(crate) const ENSURED_DIRS: &[&str] = &[
    "workspace",
    "agents",
    "data",
    "memory",
    "sessions/rollouts",
    "skills",
    "cron",
    "cron/output",
    "logs",
    "uploads",
    "cache/images",
    "cache/videos",
    "cache/audio",
];

/// 数据根下需要确保存在的空状态文件。
pub(crate) const STATE_FILES: &[(&str, &str)] = &[
    // 不预建 [mcp_servers]；统一配置文件由各设置域按需增量写入。
    ("config.toml", "# Astro configuration\n"),
    ("models.json", "{\n  \"providers\": {}\n}\n"),
    ("memory/dreaming.json", "{\n  \"enabled\": false\n}\n"),
    ("cron/jobs.json", "{\n  \"jobs\": []\n}\n"),
    ("active-agent.json", "{\n  \"id\": \"workspace\"\n}\n"),
];

/// 将模板占位符替换为实际 id 与显示名
pub(crate) fn render_template(template: &str, agent_id: &str, display_name: &str) -> String {
    template
        .replace("{{ID}}", agent_id)
        .replace("{{NAME}}", display_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_prompts_keep_distinct_responsibilities() {
        assert!(TEMPLATE_IDENTITY.contains("## 职责"));
        assert!(!TEMPLATE_IDENTITY.contains("expected_revision"));
        assert!(TEMPLATE_SOUL.contains("## 表达"));
        assert!(!TEMPLATE_SOUL.contains("notes_stale"));
        assert!(TEMPLATE_AGENTS.contains("探索 → 计划 → 实施 → 验证 → 审查修正 → 交付"));
        assert!(TEMPLATE_AGENTS.contains("notes_stale=true"));
        assert!(TEMPLATE_TOOLS.contains("## 上下文工具速查"));
        assert!(TEMPLATE_TOOLS.contains("generated/images/"));
        assert!(TEMPLATE_USER.contains("不把助手的默认规则写成用户偏好"));
        assert!(!TEMPLATE_USER.contains("复杂代码改动先列详细计划"));
        for (name, content) in CORE_FILES {
            let rendered = render_template(content, "test-agent", "Test Agent");
            assert!(!rendered.contains("{{"), "unexpanded placeholder in {name}");
        }
    }

    #[test]
    fn ensuring_workspace_preserves_existing_personalized_prompts() {
        let dir = tempfile::tempdir().unwrap();
        let ws =
            super::super::lifecycle::ensure_agent_space(dir.path(), "workspace", Some("Astro"))
                .unwrap();
        std::fs::write(ws.join("SOUL.md"), "MY_STYLE").unwrap();
        std::fs::write(ws.join("USER.md"), "MY_PREFERENCES").unwrap();
        super::super::lifecycle::ensure_agent_space(dir.path(), "workspace", Some("Astro"))
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(ws.join("SOUL.md")).unwrap(),
            "MY_STYLE"
        );
        assert_eq!(
            std::fs::read_to_string(ws.join("USER.md")).unwrap(),
            "MY_PREFERENCES"
        );
    }
}
