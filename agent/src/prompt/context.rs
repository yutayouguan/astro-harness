//! Static / Dynamic 上下文分层（对齐 Rig Agents）。
//!
//! 将人设、记忆、召回片段等拆为「每轮必带」与「按需注入」两层，
//! 由 `PromptBuilder` 或各自 `render` 方法拼入 system prompt。

/// 每轮必带的静态上下文（人设、精炼记忆、用户档案等）。
#[derive(Debug, Clone, Default)]
pub struct StaticContext {
    /// SOUL 层：核心价值观与行为准则（通常来自 `SOUL.md`）。
    pub soul: String,
    /// IDENTITY 层：对外身份与语气设定。
    pub identity: String,
    /// AGENTS 层：工作空间工作方式（`AGENTS.md`）。
    pub agent_md: String,
    /// 长期精炼记忆（`MEMORY.md` 摘要）。
    pub memory: String,
    /// 用户画像与偏好。
    pub user_profile: String,
    /// 当日日记/日文件内容。
    pub daily: String,
}

impl StaticContext {
    /// 从工作区文件内容构造上下文；`identity` 与 `agent_md` 置空，由后续填充。
    ///
    /// 适用于启动时仅加载 soul、memory、user_profile、daily 四类的场景。
    pub fn from_workspace_files(
        soul: impl Into<String>,
        memory: impl Into<String>,
        user_profile: impl Into<String>,
        daily: impl Into<String>,
    ) -> Self {
        Self {
            soul: soul.into(),
            identity: String::new(),
            agent_md: String::new(),
            memory: memory.into(),
            user_profile: user_profile.into(),
            daily: daily.into(),
        }
    }

    /// 合并为 prompt 片段（空层跳过）。
    ///
    /// 各非空层按固定标题格式化，层间以 `---` 分隔。
    pub fn render(&self) -> String {
        let mut layers = Vec::new();
        if !self.soul.trim().is_empty() {
            layers.push(format!("# 身份 / SOUL\n{}", self.soul.trim()));
        }
        if !self.identity.trim().is_empty() {
            layers.push(format!("# IDENTITY\n{}", self.identity.trim()));
        }
        if !self.agent_md.trim().is_empty() {
            layers.push(format!("# AGENT\n{}", self.agent_md.trim()));
        }
        if !self.user_profile.trim().is_empty() {
            layers.push(format!("# 用户画像\n{}", self.user_profile.trim()));
        }
        if !self.memory.trim().is_empty() {
            layers.push(format!("# 长期记忆（MEMORY.md）\n{}", self.memory.trim()));
        }
        if !self.daily.trim().is_empty() {
            layers.push(format!("# 今日记忆（流水截断）\n{}", self.daily.trim()));
        }
        layers.join("\n\n---\n\n")
    }
}

/// 按需召回的动态上下文（会话 FTS、向量 top-k 等）。
#[derive(Debug, Clone, Default)]
pub struct DynamicContext {
    /// 最多注入的片段数（对齐 Rig `dynamic_context(n, …)`）；`0` 表示不截断。
    pub max_items: usize,
    /// 按相关性排序的召回片段列表。
    pub items: Vec<String>,
}

impl DynamicContext {
    /// 创建指定上限的空动态上下文容器。
    pub fn new(max_items: usize) -> Self {
        Self {
            max_items,
            items: Vec::new(),
        }
    }

    /// 追加一条召回片段（不做去重或截断，截断在 `render` 时生效）。
    pub fn push(&mut self, item: impl Into<String>) {
        self.items.push(item.into());
    }

    /// 从单次召回文本构造上下文；空白字符串不产生条目。
    pub fn from_recalled(max_items: usize, recalled: &str) -> Self {
        let mut ctx = Self::new(max_items);
        let trimmed = recalled.trim();
        if !trimmed.is_empty() {
            ctx.push(trimmed.to_string());
        }
        ctx
    }

    /// 渲染为带标题的 prompt 块；无条目时返回空字符串。
    ///
    /// 实际取用条数为 `min(max_items, len)`，`max_items == 0` 时取全部。
    pub fn render(&self) -> String {
        if self.items.is_empty() {
            return String::new();
        }
        let take = if self.max_items == 0 {
            self.items.len()
        } else {
            self.max_items.min(self.items.len())
        };
        let body = self.items[..take].join("\n\n");
        format!("# 动态召回上下文（top-{take}）\n{body}")
    }
}
