//! Agent 主循环：会话状态、记忆召回、系统提示构建与工具调度。
//!
//! 本模块是 Astro Agent 的核心编排层，负责：
//! - 维护单次会话的消息历史与轮次预算（`max_turns` / `multi_turn`）
//! - 在每轮用户输入时召回记忆、组装静态/动态上下文并生成 system prompt
//! - 统一路由内置工具与 MCP 工具，并在调用前后触发 hooks；流式主循环在模型回复聚合后触发 `post_llm_call`
//!
//! **关键不变量**
//! - 每条用户消息开始时 `tool_rounds` 归零；工具调用次数不得超过 `multi_turn`（默认 90，对齐 Hermes）
//! - `session_messages` 中相邻消息不得连续出现相同角色（见 `validate_message_order`）
//! - 取消信号（`CancelSignal`）在工具调用前后均会检查，已取消则立即中断

use std::path::PathBuf;
use std::sync::Arc;

use uuid::Uuid;

use common::message::Message;
use memory::MemoryManager;
use ::session::{
    build_conversation_context, format_recalled_context, NewMessage, SessionStore,
};
use mcp::{is_mcp_tool_name, McpHub, MCP_TOOLSET};
use providers::registry::ProviderRegistry;
use serde_json::Value;
use tools::{dispatch_tool, register_all, ToolContext, ToolEntry, ToolRegistry};

use crate::prompt::context::{DynamicContext, StaticContext};
use crate::prompt::hooks::CancelSignal;
use crate::prompt::prompt_builder::PromptBuilder;
use crate::runtime::session::{hydrate_session_messages, resolve_session_project_root};

pub mod budget;
mod session;
pub(crate) mod usage;
mod validate;

pub use validate::validate_message_order;

/// Agent 运行时配置，控制轮次预算、记忆召回与提示组装策略。
pub struct AgentConfig {
    /// 整个会话允许的最大对话轮次（用户消息计数）。
    pub max_turns: usize,
    /// 单次用户消息内允许的工具迭代次数（对齐 Hermes `max_iterations`，默认 90）。
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
        let agent_id = home::active_agent_id(&memory_dir);
        let ws = home::agent_workspace_dir(&memory_dir, &agent_id);
        let soul = std::fs::read_to_string(ws.join("SOUL.md"))
            .unwrap_or_else(|_| "你是 Astro，一个自我进化的 AI 助手".to_string());
        Self {
            max_turns: 90,
            multi_turn: budget::DEFAULT_MAX_ITERATIONS,
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
    sessions: SessionStore,
    tool_registry: ToolRegistry,
    mcp_hub: McpHub,
    /// 最近一次 `run_turn` 召回并格式化后的记忆上下文。
    last_recalled_context: String,
    image_gen_targets: tools::ImageGenTargets,
    providers: ProviderRegistry,
    chat_api_key: String,
    chat_base_url: String,
    chat_provider: String,
    chat_model: String,
    /// 含 primary 的聊天 fallback 链（供工具/委派下传）。
    chat_targets: Vec<common::ChatTarget>,
    /// 进程内插件钩子总线（Block / Modify / Inject）。
    hook_bus: Arc<::hooks::PluginHookBus>,
    /// `pre_llm_call` 注入的本轮附加上下文（不回写用户原文）。
    pending_inject_context: Option<String>,
    cancel: CancelSignal,
    /// 代码/项目根（委派 worktree 或会话级 ASTRO_PROJECT_ROOT）。
    project_root: Option<PathBuf>,
    /// 当前多轮流式 run 的 turn_id（与 streaming `run_id` 相同）；未在 run 内为 None。
    current_turn_id: Option<String>,
    /// 同步委派执行器（由 from_memory 构造）。
    delegate_runner: delegate::DelegateRunner,
    /// 异步委派 spawner（由 from_memory 构造）。
    async_spawner: delegate::DelegateAsyncSpawner,
    /// 编排 spawner（由 from_memory 构造）。
    orchestration_spawner: orchestration::OrchestrationSpawner,
}

impl AgentLoop {
    /// 以随机 UUID 作为 session_id 创建 Agent 实例。
    pub fn new(config: AgentConfig) -> anyhow::Result<Self> {
        Self::with_session_id(config, Uuid::new_v4().to_string())
    }

    /// 以指定 session_id 创建 Agent 实例，并注册全部内置工具。
    ///
    /// 初始化时 `tool_rounds` 与 `current_turn` 均为 0。
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
        mut memory: MemoryManager,
    ) -> anyhow::Result<Self> {
        // 新 session / 构造路径：显式固化 MEMORY/USER snapshot（open 已对齐 live，此处钉死契约）。
        memory.refresh_memory_snapshot()?;
        let agent_id = memory.agent_id.clone();
        let sessions = SessionStore::open_sessions_dir(&config.memory_dir.join("sessions"))?;
        let session_messages = hydrate_session_messages(&sessions, &session_id)?;
        let mut tool_registry = ToolRegistry::new();
        register_all(&mut tool_registry);
        tool_registry.reload_enabled_from_disk(Some(&agent_id));
        let mut mcp_hub = McpHub::new();
        mcp_hub.set_agent_id(Some(agent_id));

        let orchestration_spawner: orchestration::OrchestrationSpawner = Arc::new(|req| {
            tokio::spawn(async move {
                if let Err(e) = crate::exec::orchestration::run_orchestration(req).await {
                    tracing::warn!(error = %e, "orchestration failed");
                }
            });
        });
        let delegate_runner: delegate::DelegateRunner = Arc::new(|req| {
            crate::exec::delegate::run_delegate_blocking(req)
        });
        let async_spawner: delegate::DelegateAsyncSpawner = Arc::new(|task_id, req| {
            tokio::spawn(async move {
                let reg = delegate::AsyncDelegateRegistry::global();
                if reg.is_cancel_requested(&task_id) {
                    return;
                }
                match crate::exec::delegate::run_delegate(req).await {
                    Ok(json) => {
                        if !reg.is_cancel_requested(&task_id) {
                            reg.finish_ok(&task_id, json);
                        }
                    }
                    Err(e) => {
                        if !reg.is_cancel_requested(&task_id) {
                            reg.finish_err(&task_id, e.to_string());
                        }
                    }
                }
            });
        });
        static RESUME_ONCE: std::sync::Once = std::sync::Once::new();
        let async_spawner_resume = async_spawner.clone();
        let orch_spawner_resume = orchestration_spawner.clone();
        RESUME_ONCE.call_once(move || {
            delegate::resume_incomplete_async_delegates(&async_spawner_resume);
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    if let Err(e) =
                        crate::exec::orchestration::resume_incomplete_orchestrations(&orch_spawner_resume)
                            .await
                    {
                        tracing::warn!(error = %e, "orchestration resume failed");
                    }
                });
            }
        });

        Ok(AgentLoop {
            config,
            session_id,
            current_turn: 0,
            tool_rounds: 0,
            session_messages,
            memory,
            sessions,
            tool_registry,
            mcp_hub,
            last_recalled_context: String::new(),
            image_gen_targets: tools::ImageGenTargets::default(),
            providers: ProviderRegistry::new(),
            chat_api_key: String::new(),
            chat_base_url: String::new(),
            chat_provider: String::new(),
            chat_model: String::new(),
            chat_targets: Vec::new(),
            hook_bus: Arc::new(::hooks::PluginHookBus::new()),
            pending_inject_context: None,
            cancel: CancelSignal::new(),
            project_root: resolve_session_project_root(),
            current_turn_id: None,
            delegate_runner,
            async_spawner,
            orchestration_spawner,
        })
    }

    /// 绑定当前流式 run 的 turn_id（约定与 `run_id` 相同）。
    pub fn set_current_turn_id(&mut self, turn_id: impl Into<String>) {
        self.current_turn_id = Some(turn_id.into());
    }

    /// 清除当前 turn_id（run 结束或中断时调用）。
    pub fn clear_current_turn_id(&mut self) {
        self.current_turn_id = None;
    }

    /// 当前绑定的 turn_id（若有）。
    pub fn current_turn_id(&self) -> Option<&str> {
        self.current_turn_id.as_deref()
    }

    /// 克隆同步委派执行器，供 streaming 快照使用。
    pub fn delegate_runner(&self) -> delegate::DelegateRunner {
        Arc::clone(&self.delegate_runner)
    }

    /// 克隆异步委派 spawner，供 streaming 快照使用。
    pub fn async_spawner(&self) -> delegate::DelegateAsyncSpawner {
        Arc::clone(&self.async_spawner)
    }

    /// 克隆编排 spawner，供 streaming 快照使用。
    pub fn orchestration_spawner(&self) -> orchestration::OrchestrationSpawner {
        Arc::clone(&self.orchestration_spawner)
    }

    /// 从磁盘重载 MEMORY / USER 并更新 prompt 快照（同会话写入默认不刷新）。
    pub fn refresh_memory(&mut self) -> anyhow::Result<()> {
        self.memory.refresh_memory_snapshot()
    }

    /// 设置插件钩子总线。
    pub fn set_hook_bus(&mut self, bus: Arc<::hooks::PluginHookBus>) {
        self.hook_bus = bus;
    }

    /// 当前插件钩子总线。
    pub fn hook_bus(&self) -> Arc<::hooks::PluginHookBus> {
        Arc::clone(&self.hook_bus)
    }

    /// 触发插件钩子（UI 观察由进程 `HookRuntime.ui_slot` 承接）。
    pub fn fire_hook(
        &self,
        name: &str,
        payload: ::hooks::HookPayload,
    ) -> ::hooks::HookOutcome {
        self.hook_bus.fire(name, &payload)
    }

    /// 取出并清空本轮 `pre_llm_call` 注入上下文。
    pub fn take_inject_context(&mut self) -> Option<String> {
        self.pending_inject_context.take()
    }

    /// 当前会话轮次序号（从 1 起，未开始为 0）。
    pub fn session_turn(&self) -> usize {
        self.current_turn
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
    pub fn set_image_gen_targets(&mut self, targets: tools::ImageGenTargets) {
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

    /// 设置代码/项目根（委派 worktree）；`None` 时文件/终端回退到记忆工作区。
    pub fn set_project_root(&mut self, root: Option<PathBuf>) {
        self.project_root = root;
    }

    /// 当前代码/项目根（若有）。
    pub fn project_root(&self) -> Option<&PathBuf> {
        self.project_root.as_ref()
    }

    /// 设置含 primary 的聊天 fallback 链（主聊 / cron / delegate 共用）。
    pub fn set_chat_targets(&mut self, targets: Vec<common::ChatTarget>) {
        self.chat_targets = targets;
    }

    /// 当前聊天 fallback 链。
    pub fn chat_targets(&self) -> &[common::ChatTarget] {
        &self.chat_targets
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

    /// 记忆根目录。
    pub fn memory_dir(&self) -> &std::path::Path {
        &self.config.memory_dir
    }

    /// 与本 Agent 消息落盘共用的会话库。
    pub fn sessions(&self) -> &SessionStore {
        &self.sessions
    }

    /// 当前 Agent 工作区路径。
    pub fn workspace_dir(&self) -> std::path::PathBuf {
        self.memory.workspace_dir.clone()
    }

    pub fn chat_api_key(&self) -> &str {
        &self.chat_api_key
    }

    pub fn chat_base_url(&self) -> &str {
        &self.chat_base_url
    }

    pub fn chat_provider(&self) -> &str {
        &self.chat_provider
    }

    pub fn chat_model(&self) -> &str {
        &self.chat_model
    }

    pub fn image_gen_targets(&self) -> &tools::ImageGenTargets {
        &self.image_gen_targets
    }

    /// Provider 注册表的共享副本（并发工具快照用）。
    pub fn providers_arc(&self) -> Arc<ProviderRegistry> {
        Arc::new(self.providers.clone())
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

    /// 与 `build_system_prompt` 同源加载静态/动态上下文与技能列表（不含 env 副作用）。
    fn system_prompt_parts(&self) -> (StaticContext, DynamicContext, Vec<(String, String)>) {
        let (project_memory, user_profile, daily) = self.memory.prompt_snapshot_with_daily();
        let skill_pairs = if self.tool_registry.is_toolset_enabled("skills") {
            skills::list_enabled_for_prompt()
        } else {
            Vec::new()
        };

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
        (static_ctx, dynamic_ctx, skill_pairs)
    }

    /// 组装完整 system prompt：静态上下文 + 动态召回 + 技能索引 + 工具指引 + 时间戳。
    ///
    /// MEMORY / USER 仅注入 **snapshot**（同会话冻结）；日记读盘后截断注入。
    /// 副作用：设置 `ASTRO_WORKSPACE` 环境变量供工具读取。
    pub fn build_system_prompt(&self) -> String {
        let (static_ctx, dynamic_ctx, skill_pairs) = self.system_prompt_parts();
        std::env::set_var("ASTRO_WORKSPACE", &self.memory.workspace_dir);
        let skill_index: Vec<(&str, &str)> = skill_pairs
            .iter()
            .map(|(name, desc)| (name.as_str(), desc.as_str()))
            .collect();

        PromptBuilder::new()
            .with_static_context(&static_ctx)
            .with_dynamic_context(&dynamic_ctx)
            .with_skills_index(&skill_index)
            .with_tool_guidance()
            .with_timestamp()
            .build()
    }

    /// 与 `build_system_prompt` 同源的分层字符数，供上下文占用估算。
    /// 返回 (system, memory, skills, recall)。
    pub fn system_prompt_layer_chars(&self) -> (usize, usize, usize, usize) {
        let (static_ctx, dynamic_ctx, skill_pairs) = self.system_prompt_parts();
        let skill_index: Vec<(&str, &str)> = skill_pairs
            .iter()
            .map(|(name, desc)| (name.as_str(), desc.as_str()))
            .collect();

        let guidance_ts = PromptBuilder::new().with_tool_guidance().with_timestamp().build();
        let mut system_chars = guidance_ts.len();
        for part in [&static_ctx.soul, &static_ctx.identity, &static_ctx.agent_md] {
            system_chars += part.trim().len();
        }
        let memory_chars = static_ctx.memory.trim().len()
            + static_ctx.user_profile.trim().len()
            + static_ctx.daily.trim().len();
        let skills_chars = PromptBuilder::new().with_skills_index(&skill_index).build().len();
        let recall_chars = dynamic_ctx.render().len();
        (system_chars, memory_chars, skills_chars, recall_chars)
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
            let turn_id = self.current_turn_id.clone();
            let _ = home::record_tool_call(&agent_id, name, args);
            let _ = ::usage::record_tool_call(
                &agent_id,
                name,
                args,
                Some(self.session_id.as_str()),
                turn_id.as_deref(),
            );
            ::usage::UsageDb::try_record(::usage::NewUsageEvent {
                ts: chrono::Utc::now().to_rfc3339(),
                kind: "mcp".into(),
                name: name.to_string(),
                agent_id,
                session_id: Some(self.session_id.clone()),
                turn_id,
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                reasoning_tokens: 0,
                total_tokens: 0,
                cost_usd: 0.0,
                cost_status: None,
                cost_source: None,
                pricing_version: None,
                billing_provider: None,
                billing_base_url: None,
                billing_mode: None,
                meta_json: None,
            });
            return self.mcp_hub.call_tool(name, args).await;
        }

        let allowed = self.tool_registry.is_tool_allowed(name);
        let workspace_dir = self.resolve_workspace_dir();
        std::env::set_var("ASTRO_WORKSPACE", &workspace_dir);
        let image_gen_targets = self.image_gen_targets.clone();
        let session_id = self.session_id.clone();
        let turn_id = self.current_turn_id.clone();
        let chat_api_key = self.chat_api_key.clone();
        let chat_base_url = self.chat_base_url.clone();
        let chat_provider = self.chat_provider.clone();
        let chat_model = self.chat_model.clone();
        let chat_targets = self.chat_targets.clone();
        let memory_dir = self.config.memory_dir.clone();
        let sessions = &self.sessions;
        let delegate_runner = Some(self.delegate_runner());
        let async_spawner = Some(self.async_spawner());
        let orchestration_spawner = Some(self.orchestration_spawner());
        let mut ctx = ToolContext {
            memory: &mut self.memory,
            sessions,
            memory_dir,
            workspace_dir,
            project_root: self.project_root.clone(),
            image_gen_targets: &image_gen_targets,
            providers: &self.providers,
            session_id,
            turn_id,
            chat_api_key,
            chat_base_url,
            chat_provider,
            chat_model,
            chat_targets,
            delegate_runner,
            async_spawner,
            orchestration_spawner,
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
        // 可拦截：PluginHookBus 优先
        let bus_out = self.fire_hook(
            ::hooks::PRE_TOOL_CALL,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id: self.current_turn_id.clone(),
                tool_name: Some(name.into()),
                tool_args: Some(args.clone()),
                detail: format!("{name} {args}"),
                ..Default::default()
            },
        );
        let mut args_owned = args.clone();
        match bus_out {
            ::hooks::HookOutcome::Block(reason) => {
                return Ok(format!("[blocked by hook] {reason}"));
            }
            ::hooks::HookOutcome::Modify(v) => {
                args_owned = v;
            }
            _ => {}
        }
        if self.cancel.is_cancelled() {
            anyhow::bail!("prompt cancelled");
        }
        let result = self.dispatch_named_tool(name, &args_owned).await?;
        let _ = self.fire_hook(
            ::hooks::POST_TOOL_CALL,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id: self.current_turn_id.clone(),
                tool_name: Some(name.into()),
                tool_result: Some(result.clone()),
                detail: {
                    let preview: String = result.chars().take(200).collect();
                    format!("{name} → {preview}")
                },
                ..Default::default()
            },
        );
        if name == "delegate" || name == "multi_agent" {
            self.fire_subagent_stop_from_delegate_result(&result).await;
        }
        Ok(result)
    }

    async fn fire_subagent_stop_from_delegate_result(&self, result: &str) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(result) else {
            return;
        };
        let tasks = v
            .get("tasks")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_else(|| {
                if v.get("session_id").is_some() {
                    vec![v.clone()]
                } else {
                    Vec::new()
                }
            });
        for t in tasks {
            let child = t
                .get("session_id")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown");
            let summary = t
                .get("summary")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            let _ = self.fire_hook(
                ::hooks::SUBAGENT_STOP,
                ::hooks::HookPayload {
                    session_id: child.into(),
                    turn_id: self.current_turn_id.clone(),
                    detail: summary.chars().take(200).collect(),
                    ..Default::default()
                },
            );
        }
    }

    /// 确保会话行存在（不存在则按 `source` 创建）。
    pub fn ensure_session(&self, source: &str) -> anyhow::Result<()> {
        self.sessions.ensure_session(&self.session_id, source)
    }

    /// 将 assistant 纯文本回复写入记忆与会话镜像。
    pub fn record_assistant_message(&mut self, content: &str) -> anyhow::Result<()> {
        self.record_assistant_message_with_tools(content, None, None, None)
    }

    /// 将 assistant 回复（可含 tool_calls / reasoning / reasoning_details）写入记忆与会话镜像。
    ///
    /// 非空 `tool_calls` 时使用 `Message::assistant_with_tools` 保留结构化调用信息；
    /// 落盘通过 `SessionStore::append_message` 写入富字段。
    pub fn record_assistant_message_with_tools(
        &mut self,
        content: &str,
        tool_calls: Option<Vec<common::message::ToolCall>>,
        reasoning: Option<&str>,
        reasoning_details: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let tool_calls_json = match &tool_calls {
            Some(calls) if !calls.is_empty() => Some(serde_json::to_value(calls)?),
            _ => None,
        };
        let reasoning = reasoning.filter(|r| !r.is_empty());
        self.sessions.ensure_session(&self.session_id, "tauri")?;
        self.sessions.append_message(NewMessage {
            content: Some(content),
            tool_calls: tool_calls_json,
            reasoning,
            reasoning_details,
            ..NewMessage::empty(&self.session_id, "assistant")
        })?;
        let msg = match tool_calls {
            Some(calls) if !calls.is_empty() => Message::assistant_with_tools(content, calls),
            _ => Message::assistant(content),
        };
        self.session_messages.push(msg);
        Ok(())
    }

    /// 将 tool 角色结果写入记忆与会话镜像（无 tool_call_id / tool_name）。
    pub fn record_tool_result(&mut self, content: &str) -> anyhow::Result<()> {
        self.record_tool_result_with_id(None, None, content)
    }

    /// 将 tool 角色结果写入记忆与会话镜像，并关联 `tool_call_id` / `tool_name`。
    pub fn record_tool_result_with_id(
        &mut self,
        tool_call_id: Option<&str>,
        tool_name: Option<&str>,
        content: &str,
    ) -> anyhow::Result<()> {
        self.sessions.ensure_session(&self.session_id, "tauri")?;
        self.sessions.append_message(NewMessage {
            content: Some(content),
            tool_call_id,
            tool_name,
            ..NewMessage::empty(&self.session_id, "tool")
        })?;
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
        self.run_turn_with_images(user_message, &[], _task_id).await
    }

    /// 同 [`Self::run_turn`]，附带本轮图片 data URL（`data:image/...;base64,...`）。
    ///
    /// FTS / `record_message` 仍只记文本；图片只进内存 `session_messages` 的 Parts。
    pub async fn run_turn_with_images(
        &mut self,
        user_message: &str,
        image_data_urls: &[String],
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

        self.sessions.ensure_session(&self.session_id, "tauri")?;
        self.sessions.append_message(NewMessage {
            content: Some(user_message),
            ..NewMessage::empty(&self.session_id, "user")
        })?;

        let fts_keywords = if self.current_turn >= self.config.recent_turns {
            Some(user_message)
        } else {
            None
        };
        let recalled = build_conversation_context(
            &self.sessions,
            &self.session_id,
            self.config.recent_turns,
            fts_keywords,
        )?;
        self.last_recalled_context = format_recalled_context(&recalled);

        self.session_messages
            .push(Message::user_with_images(user_message, image_data_urls));
        self.increment_turn();
        let system_prompt = self.build_system_prompt();
        let _ = self.fire_hook(
            ::hooks::ON_SESSION_START,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id: self.current_turn_id.clone(),
                detail: format!("session={}", self.session_id),
                ..Default::default()
            },
        );
        let inject = self.fire_hook(
            ::hooks::PRE_LLM_CALL,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id: self.current_turn_id.clone(),
                system_prompt_chars: Some(system_prompt.len()),
                detail: format!("system_prompt_chars={}", system_prompt.len()),
                ..Default::default()
            },
        );
        if let ::hooks::HookOutcome::InjectContext(ctx) = inject {
            self.pending_inject_context = Some(ctx);
        }
        if self.cancel.is_cancelled() {
            return Ok(TurnResult::Interrupted);
        }
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

