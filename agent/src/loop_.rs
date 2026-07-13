//! Agent 主循环：会话状态、记忆召回、系统提示构建与工具调度。
//!
//! 本模块是 Astro Agent 的核心编排层，负责：
//! - 维护单次会话的消息历史与轮次预算（`max_turns` / `multi_turn`）
//! - 在每轮用户输入时召回记忆、组装静态/动态上下文并生成 system prompt
//! - 统一路由内置工具与 MCP 工具，并在调用前后触发 hooks；流式主循环在模型回复聚合后触发 `on_completion`
//!
//! **关键不变量**
//! - 每条用户消息开始时 `tool_rounds` 归零；工具调用次数不得超过 `multi_turn`
//! - `session_messages` 中相邻消息不得连续出现相同角色（见 `validate_message_order`）
//! - 取消信号（`CancelSignal`）在工具调用前后均会检查，已取消则立即中断

use std::path::PathBuf;
use std::sync::Arc;

use uuid::Uuid;

use common::message::Message;
use memory::{format_recalled_context, MemoryManager};
use mcp::{is_mcp_tool_name, McpHub, MCP_TOOLSET};
use providers::registry::ProviderRegistry;
use serde_json::Value;
use tools::{dispatch_tool, register_all, ToolContext, ToolEntry, ToolRegistry};

use crate::context::{DynamicContext, StaticContext};
use crate::hooks::{CancelSignal, NoopHooks, PromptHooks};
use crate::prompt_builder::PromptBuilder;

/// 图像生成凭据与输出目标，供 `image_gen` 等工具使用。
pub use tools::{ImageGenCreds, ImageGenTargets};

/// Agent 运行时配置，控制轮次预算、记忆召回与提示组装策略。
pub struct AgentConfig {
    /// 整个会话允许的最大对话轮次（用户消息计数）。
    pub max_turns: usize,
    /// 单次用户消息内允许的工具轮次（对齐 Rig multi_turn）。
    pub multi_turn: usize,
    /// 上下文压缩时保留的最近消息条数。
    pub protect_last_n: usize,
    /// 记忆与工作区根目录。
    pub memory_dir: PathBuf,
    /// 近期对话窗口大小，用于 FTS 召回触发阈值。
    pub recent_turns: usize,
    /// Agent 人格描述（通常来自 `SOUL.md`）。
    pub soul: String,
    /// LLM 采样温度。
    pub temperature: f32,
    /// 透传给 Provider 的额外 JSON 参数。
    pub additional_params: Value,
    /// 动态上下文（召回记忆）最大条目数。
    pub dynamic_max_items: usize,
    /// 可选的静态上下文覆盖，用于测试或自定义 prompt。
    pub static_override: Option<StaticContext>,
}

impl AgentConfig {
    /// 以工作区默认值构造配置：读取 `SOUL.md` 并确保记忆目录存在。
    ///
    /// 不变量：`memory_dir` 必须可写；`ensure_workspace` 失败时仍继续，使用内置默认 soul。
    pub fn with_defaults(memory_dir: PathBuf) -> Self {
        let _ = memory::ensure_workspace(&memory_dir);
        let agent_id = memory::active_agent_id(&memory_dir);
        let ws = memory::agent_workspace_dir(&memory_dir, &agent_id);
        let soul = std::fs::read_to_string(ws.join("SOUL.md"))
            .unwrap_or_else(|_| "你是 Astro，一个自我进化的 AI 助手".to_string());
        Self {
            max_turns: 90,
            multi_turn: 8,
            protect_last_n: 20,
            memory_dir,
            recent_turns: 10,
            soul,
            temperature: 0.7,
            additional_params: Value::Null,
            dynamic_max_items: 3,
            static_override: None,
        }
    }
}

/// Agent 主循环状态机：持有会话、记忆、工具注册表与 Provider 凭据。
///
/// 生命周期：通过 `run_turn` 处理用户输入，通过 `handle_tool_call_async` 执行工具，
/// 由上层 streaming 层驱动 LLM 往返。
pub struct AgentLoop {
    config: AgentConfig,
    session_id: String,
    /// 当前会话已消耗的用户轮次数。
    current_turn: usize,
    /// 当前用户消息内已使用的工具轮次。
    tool_rounds: usize,
    /// 内存中的会话消息镜像，与磁盘记忆同步追加。
    pub session_messages: Vec<Message>,
    memory: MemoryManager,
    tool_registry: ToolRegistry,
    mcp_hub: McpHub,
    /// 最近一次 `run_turn` 召回并格式化后的记忆上下文。
    last_recalled_context: String,
    image_gen_targets: ImageGenTargets,
    providers: ProviderRegistry,
    chat_api_key: String,
    chat_base_url: String,
    chat_provider: String,
    chat_model: String,
    hooks: Arc<dyn PromptHooks>,
    cancel: CancelSignal,
}

impl AgentLoop {
    /// 以随机 UUID 作为 session_id 创建 Agent 实例。
    pub fn new(config: AgentConfig) -> anyhow::Result<Self> {
        Self::with_session_id(config, Uuid::new_v4().to_string())
    }

    /// 以指定 session_id 创建 Agent 实例，并注册全部内置工具。
    ///
    /// 初始化时 `tool_rounds` 与 `current_turn` 均为 0，hooks 默认为 `NoopHooks`。
    /// 记忆侧使用当前活跃 Agent（[`MemoryManager::new`]）。
    pub fn with_session_id(config: AgentConfig, session_id: String) -> anyhow::Result<Self> {
        let memory = MemoryManager::new(config.memory_dir.clone())?;
        Self::from_memory(config, session_id, memory)
    }

    /// 以指定 `agent_id` 与 session_id 创建 Agent 实例（不依赖全局活跃 Agent）。
    pub fn with_session_id_for_agent(
        config: AgentConfig,
        session_id: String,
        agent_id: &str,
    ) -> anyhow::Result<Self> {
        let memory = MemoryManager::for_agent(config.memory_dir.clone(), agent_id)?;
        Self::from_memory(config, session_id, memory)
    }

    fn from_memory(
        config: AgentConfig,
        session_id: String,
        memory: MemoryManager,
    ) -> anyhow::Result<Self> {
        let agent_id = memory.agent_id.clone();
        let mut tool_registry = ToolRegistry::new();
        register_all(&mut tool_registry);
        tool_registry.reload_enabled_from_disk(Some(&agent_id));
        let mut mcp_hub = McpHub::new();
        mcp_hub.set_agent_id(Some(agent_id));
        ensure_orchestration_spawner_registered();
        Ok(AgentLoop {
            config,
            session_id,
            current_turn: 0,
            tool_rounds: 0,
            session_messages: Vec::new(),
            memory,
            tool_registry,
            mcp_hub,
            last_recalled_context: String::new(),
            image_gen_targets: ImageGenTargets::default(),
            providers: ProviderRegistry::new(),
            chat_api_key: String::new(),
            chat_base_url: String::new(),
            chat_provider: String::new(),
            chat_model: String::new(),
            hooks: Arc::new(NoopHooks),
            cancel: CancelSignal::new(),
        })
    }

    /// 注入生命周期 hooks（工具调用、prompt 构建、轮次结束等回调）。
    pub fn set_hooks(&mut self, hooks: Arc<dyn PromptHooks>) {
        self.hooks = hooks;
    }

    /// 克隆当前 hooks，供 streaming 在不持有 `AgentLoop` 借用时触发回调。
    pub fn prompt_hooks(&self) -> Arc<dyn PromptHooks> {
        Arc::clone(&self.hooks)
    }

    /// 返回可克隆的取消信号，供上层 streaming 或 UI 触发中断。
    pub fn cancel_signal(&self) -> CancelSignal {
        self.cancel.clone()
    }

    /// 单次用户消息内允许的最大工具轮次。
    pub fn multi_turn(&self) -> usize {
        self.config.multi_turn
    }

    /// 当前 LLM 采样温度。
    pub fn temperature(&self) -> f32 {
        self.config.temperature
    }

    /// 透传给 Provider 的额外 JSON 参数引用。
    pub fn additional_params(&self) -> &Value {
        &self.config.additional_params
    }

    /// 当前用户消息的工具深度是否已达 `multi_turn` 上限。
    pub fn is_tool_depth_exhausted(&self) -> bool {
        self.tool_rounds >= self.config.multi_turn
    }

    /// 开始新的用户消息处理：重置 `tool_rounds` 为 0。
    pub fn begin_user_turn(&mut self) {
        self.tool_rounds = 0;
    }

    /// 递增工具轮次计数；超出 `multi_turn` 时返回 [`MaxDepthError`]。
    pub fn increment_tool_round(&mut self) -> Result<(), MaxDepthError> {
        if self.is_tool_depth_exhausted() {
            return Err(MaxDepthError {
                limit: self.config.multi_turn,
                used: self.tool_rounds,
            });
        }
        self.tool_rounds += 1;
        Ok(())
    }

    /// 设置图像生成工具的输出目标路径。
    pub fn set_image_gen_targets(&mut self, targets: ImageGenTargets) {
        self.image_gen_targets = targets;
    }

    /// 配置 LLM 对话凭据，供需要调用 Provider 的内置工具使用。
    pub fn set_chat_credentials(
        &mut self,
        provider: &str,
        model: &str,
        api_key: &str,
        base_url: &str,
    ) {
        self.chat_provider = provider.to_string();
        self.chat_model = model.to_string();
        self.chat_api_key = api_key.to_string();
        self.chat_base_url = base_url.to_string();
    }

    /// 返回 `(project_memory, user_profile)` 原始 prompt 片段。
    pub fn prompt_content(&self) -> (String, String) {
        self.memory.prompt_content()
    }

    /// 当前会话唯一标识符。
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// 当前 Agent 标识（来自 MemoryManager）。
    pub fn agent_id(&self) -> &str {
        &self.memory.agent_id
    }

    /// 内置与 MCP 工具的注册表只读引用。
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }

    /// 工具注册表可变引用（编排子步剔除 `orchestration_*` 等）。
    pub fn tool_registry_mut(&mut self) -> &mut ToolRegistry {
        &mut self.tool_registry
    }

    /// MCP Hub 只读引用，用于外部查询或调试。
    pub fn mcp_hub(&self) -> &McpHub {
        &self.mcp_hub
    }

    /// 从磁盘重载当前 Agent 的工具启用开关（gate 配置）。
    pub fn reload_tool_gates(&mut self) {
        let agent_id = self.memory.agent_id.clone();
        self.tool_registry
            .reload_enabled_from_disk(Some(&agent_id));
    }

    /// 从磁盘重载 MCP 配置，并将启用工具挂接到 [`ToolRegistry`]。
    ///
    /// 失败时仅记录 warn 日志，不中断调用方；成功后 MCP 工具以 `MCP_TOOLSET` 注册。
    pub async fn reload_mcp(&mut self) {
        let agent_id = self.memory.agent_id.clone();
        if let Err(e) = self.mcp_hub.reload_from_disk(Some(&agent_id)).await {
            tracing::warn!(error = %e, "reload MCP failed");
        }
        self.attach_mcp_tools();
    }

    /// 同时重载工具 gate 与 MCP 配置，通常在每轮用户输入开始时调用。
    pub async fn reload_tools_and_mcp(&mut self) {
        self.reload_tool_gates();
        self.reload_mcp().await;
    }

    /// 将 MCP Hub 中已启用的工具条目同步到 [`ToolRegistry`]。
    ///
    /// 先卸载旧 `MCP_TOOLSET` 再逐条注册，保证与磁盘 enablement 一致。
    fn attach_mcp_tools(&mut self) {
        self.tool_registry.unregister_toolset(MCP_TOOLSET);
        for spec in self.mcp_hub.enabled_tool_entries() {
            self.tool_registry.register(ToolEntry {
                name: spec.qualified_name,
                toolset: MCP_TOOLSET.to_string(),
                description: spec.description,
                schema: spec.schema,
                check_fn: None,
                icon: "plug",
            });
        }
    }

    /// 最近一次记忆召回的格式化文本，已注入动态上下文。
    pub fn recalled_context(&self) -> &str {
        &self.last_recalled_context
    }

    /// 生成新的任务 UUID，供上层追踪单次 LLM 请求。
    pub fn new_task_id(&self) -> String {
        Uuid::new_v4().to_string()
    }

    /// 会话轮次预算是否已耗尽（`current_turn >= max_turns`）。
    pub fn is_budget_exhausted(&self) -> bool {
        self.current_turn >= self.config.max_turns
    }

    /// 递增会话轮次计数（每处理一条用户消息调用一次）。
    pub fn increment_turn(&mut self) {
        self.current_turn += 1;
    }

    /// 根据上下文占用比例判断是否需要触发压缩。
    ///
    /// 当前阈值：占用超过 50% 时返回 `true`。
    pub fn needs_compression(&self, context_ratio: f32) -> bool {
        context_ratio > 0.5
    }

    /// 组装完整 system prompt：静态上下文 + 动态召回 + 技能索引 + 工具指引 + 时间戳。
    ///
    /// 副作用：设置 `ASTRO_WORKSPACE` 环境变量供工具读取。
    pub fn build_system_prompt(&self) -> String {
        let (project_memory, user_profile, daily) = self.memory.prompt_content_with_daily();
        std::env::set_var("ASTRO_WORKSPACE", &self.memory.workspace_dir);
        let skill_pairs = if self.tool_registry.is_toolset_enabled("skills") {
            skills::list_enabled_for_prompt()
        } else {
            Vec::new()
        };
        let skill_index: Vec<(&str, &str)> = skill_pairs
            .iter()
            .map(|(name, desc)| (name.as_str(), desc.as_str()))
            .collect();

        let static_ctx = if let Some(ref over) = self.config.static_override {
            over.clone()
        } else {
            StaticContext::from_workspace_files(
                &self.config.soul,
                &project_memory,
                &user_profile,
                &daily,
            )
        };
        let dynamic_ctx = DynamicContext::from_recalled(
            self.config.dynamic_max_items,
            &self.last_recalled_context,
        );

        PromptBuilder::new()
            .with_static_context(&static_ctx)
            .with_dynamic_context(&dynamic_ctx)
            .with_skills_index(&skill_index)
            .with_tool_guidance()
            .with_timestamp()
            .build()
    }

    /// 解析当前 Agent 工作区目录，供工具上下文注入。
    fn resolve_workspace_dir(&self) -> PathBuf {
        self.memory.workspace_dir.clone()
    }

    /// 按名称分发工具调用：MCP 走 Hub，内置工具走 [`dispatch_tool`]。
    ///
    /// 调用前刷新 gate 与 MCP 注册；未启用或不存在的工具直接 bail。
    async fn dispatch_named_tool(
        &mut self,
        name: &str,
        args: &serde_json::Value,
    ) -> anyhow::Result<String> {
        let agent_id = self.memory.agent_id.clone();
        self.tool_registry
            .reload_enabled_from_disk(Some(&agent_id));

        if is_mcp_tool_name(name) {
            let _ = self.mcp_hub.sync_enablement_from_disk();
            self.attach_mcp_tools();
            if !self.tool_registry.is_tool_allowed(name) {
                anyhow::bail!("MCP 工具未启用或不存在: {name}");
            }
            let agent_id = self.memory.agent_id.clone();
            let _ = memory::record_tool_call(&agent_id, name, args);
            let _ = memory::record_usage_tool_call(&agent_id, name, args);
            memory::UsageDb::try_record(memory::NewUsageEvent {
                ts: chrono::Utc::now().to_rfc3339(),
                kind: "mcp".into(),
                name: name.to_string(),
                agent_id,
                session_id: Some(self.session_id.clone()),
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
                cost_usd: 0.0,
                meta_json: None,
            });
            return self.mcp_hub.call_tool(name, args).await;
        }

        let allowed = self.tool_registry.is_tool_allowed(name);
        let workspace_dir = self.resolve_workspace_dir();
        std::env::set_var("ASTRO_WORKSPACE", &workspace_dir);
        let image_gen_targets = self.image_gen_targets.clone();
        let session_id = self.session_id.clone();
        let chat_api_key = self.chat_api_key.clone();
        let chat_base_url = self.chat_base_url.clone();
        let chat_provider = self.chat_provider.clone();
        let chat_model = self.chat_model.clone();
        let memory_dir = self.config.memory_dir.clone();
        let mut ctx = ToolContext {
            memory: &mut self.memory,
            memory_dir,
            workspace_dir,
            image_gen_targets: &image_gen_targets,
            providers: &self.providers,
            session_id,
            chat_api_key,
            chat_base_url,
            chat_provider,
            chat_model,
        };
        dispatch_tool(|_| allowed, &mut ctx, name, args).await
    }

    /// 同步执行工具调用：在无 tokio runtime 时自建 current_thread runtime。
    ///
    /// 适用于 Tauri 等同步边界；异步上下文优先使用 [`handle_tool_call_async`]。
    pub fn handle_tool_call(
        &mut self,
        name: &str,
        args: &serde_json::Value,
    ) -> anyhow::Result<String> {
        let fut = self.handle_tool_call_async(name, args);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.block_on(fut)
        } else {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(fut)
        }
    }

    /// 异步执行单次工具调用：检查取消 → 递增深度 → hooks → 分发 → hooks。
    ///
    /// 取消或深度耗尽时返回错误；成功时返回工具输出字符串。
    pub async fn handle_tool_call_async(
        &mut self,
        name: &str,
        args: &serde_json::Value,
    ) -> anyhow::Result<String> {
        if self.cancel.is_cancelled() {
            anyhow::bail!("prompt cancelled");
        }
        self.increment_tool_round()?;
        self.hooks.on_tool_call(name, args, &self.cancel).await;
        if self.cancel.is_cancelled() {
            anyhow::bail!("prompt cancelled");
        }
        let result = self.dispatch_named_tool(name, args).await?;
        self.hooks
            .on_tool_result(name, &result, &self.cancel)
            .await;
        Ok(result)
    }

    /// 将 assistant 纯文本回复写入记忆与会话镜像。
    pub fn record_assistant_message(&mut self, content: &str) -> anyhow::Result<()> {
        self.record_assistant_message_with_tools(content, None)
    }

    /// 将 assistant 回复（可含 tool_calls）写入记忆与会话镜像。
    ///
    /// 非空 `tool_calls` 时使用 `Message::assistant_with_tools` 保留结构化调用信息。
    pub fn record_assistant_message_with_tools(
        &mut self,
        content: &str,
        tool_calls: Option<Vec<common::message::ToolCall>>,
    ) -> anyhow::Result<()> {
        self.memory
            .record_message(&self.session_id, "assistant", content)?;
        let msg = match tool_calls {
            Some(calls) if !calls.is_empty() => Message::assistant_with_tools(content, calls),
            _ => Message::assistant(content),
        };
        self.session_messages.push(msg);
        Ok(())
    }

    /// 将 tool 角色结果写入记忆与会话镜像（无 tool_call_id）。
    pub fn record_tool_result(&mut self, content: &str) -> anyhow::Result<()> {
        self.record_tool_result_with_id(None, content)
    }

    /// 将 tool 角色结果写入记忆与会话镜像，并关联 `tool_call_id`。
    pub fn record_tool_result_with_id(
        &mut self,
        tool_call_id: Option<&str>,
        content: &str,
    ) -> anyhow::Result<()> {
        self.memory
            .record_message(&self.session_id, "tool", content)?;
        let msg = match tool_call_id {
            Some(id) if !id.is_empty() => Message::tool_with_id(id, content),
            _ => Message::tool(content),
        };
        self.session_messages.push(msg);
        Ok(())
    }

    /// 处理一轮用户输入：记录消息、召回记忆、构建 system prompt。
    ///
    /// 返回 [`TurnResult::Continue`] 供上层发起 LLM 请求；预算耗尽或已取消时提前返回。
    /// 注意：本方法不直接调用 LLM，仅完成 Agent 侧准备工作。
    pub async fn run_turn(
        &mut self,
        user_message: &str,
        _task_id: &str,
    ) -> anyhow::Result<TurnResult> {
        if self.cancel.is_cancelled() {
            return Ok(TurnResult::Interrupted);
        }
        if self.is_budget_exhausted() {
            return Ok(TurnResult::BudgetExhausted);
        }

        self.begin_user_turn();
        self.reload_tools_and_mcp().await;

        self.memory
            .record_message(&self.session_id, "user", user_message)?;

        let fts_keywords = if self.current_turn >= self.config.recent_turns {
            Some(user_message)
        } else {
            None
        };
        let recalled = self.memory.build_session_context(
            &self.session_id,
            self.config.recent_turns,
            fts_keywords,
        )?;
        self.last_recalled_context = format_recalled_context(&recalled);

        self.session_messages.push(Message::user(user_message));
        self.increment_turn();
        let system_prompt = self.build_system_prompt();
        self.hooks
            .on_prompt_build(&system_prompt, &self.cancel)
            .await;
        if self.cancel.is_cancelled() {
            return Ok(TurnResult::Interrupted);
        }
        self.hooks
            .on_turn_end(self.current_turn, &self.cancel)
            .await;
        Ok(TurnResult::Continue {
            turn: self.current_turn,
            system_prompt,
        })
    }
}

/// 工具循环超过 `multi_turn` 限制时抛出的错误（对齐 Rig `MaxDepthError`）。
#[derive(Debug, Clone)]
pub struct MaxDepthError {
    /// 配置的上限轮次。
    pub limit: usize,
    /// 已消耗的轮次（触发错误时尚未递增）。
    pub used: usize,
}

impl std::fmt::Display for MaxDepthError {
    /// 格式化错误信息：`used / limit`。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "tool multi_turn exhausted: used {} / limit {}",
            self.used, self.limit
        )
    }
}

impl std::error::Error for MaxDepthError {}

/// 单轮 `run_turn` 或上层编排的可能结果。
#[derive(Debug)]
pub enum TurnResult {
    /// 准备就绪，携带轮次编号与 system prompt，等待 LLM 响应。
    Continue { turn: usize, system_prompt: String },
    /// 模型请求的工具名称列表（由 streaming 层填充）。
    ToolCalls(Vec<String>),
    /// 对话自然结束，携带最终 assistant 文本。
    Finished(String),
    /// 会话 `max_turns` 预算已耗尽。
    BudgetExhausted,
    /// 工具深度 `multi_turn` 已耗尽。
    MaxDepth,
    /// 用户或上层触发了取消。
    Interrupted,
}

/// 校验消息序列是否满足「相邻不同角色」约束。
///
/// 连续两条 user 或连续两条 assistant 均视为非法，返回 `false`。
pub fn validate_message_order(messages: &[Message]) -> bool {
    use common::message::Role;
    for window in messages.windows(2) {
        let (a, b) = (&window[0], &window[1]);
        if a.role == Role::User && b.role == Role::User {
            return false;
        }
        if a.role == Role::Assistant && b.role == Role::Assistant {
            return false;
        }
    }
    true
}

/// 注册编排 spawner（OnceLock，仅首次生效）。由 tools 落库后回调。
fn ensure_orchestration_spawner_registered() {
    memory::set_orchestration_spawner(Arc::new(|req| {
        tokio::spawn(async move {
            if let Err(e) = crate::orchestration::run_orchestration(req).await {
                tracing::warn!(error = %e, "orchestration failed");
            }
        });
    }));
}
