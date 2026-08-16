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

use tokio::sync::Mutex as TokioMutex;
use uuid::Uuid;

use ::session::{ConversationStore, SessionStore};
use mcp::{McpHub, MCP_TOOLSET};
use memory::MemoryManager;
use serde_json::Value;
use tools::{register_all, ToolRegistry};
use types::message::Message;
use types::ToolEntry;

use crate::prompt::context::StaticContext;
use crate::prompt::hooks::CancelSignal;
use crate::runtime::session::{hydrate_session_messages, resolve_session_project_root};

pub mod budget;
pub(crate) mod compression_state;
mod context_maintenance;
pub(crate) mod model_ctx;
mod recording;
mod session;
mod system_prompt;
mod tool_dispatch;
pub(crate) mod turn_budget;
mod turn_lifecycle;
pub(crate) mod usage;
mod validate;

pub use tool_dispatch::ToolCallError;
pub use turn_budget::MaxDepthError;
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
    /// System prompt 总字符预算（[`crate::prompt::ContextBudget`]）；默认宽裕。
    pub context_budget_chars: usize,
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
            context_budget_chars: crate::prompt::DEFAULT_CONTEXT_BUDGET_CHARS,
            static_override: None,
        }
    }
}

/// Agent 主循环状态机：持有会话、记忆、工具注册表与 Provider 凭据。
///
/// 生命周期：通过 `run_turn` 处理用户输入，通过 `handle_tool_call_async` 执行工具，
/// 由上层 streaming 层驱动 LLM 往返。
pub struct AgentLoop {
    pub(crate) config: AgentConfig,
    pub(crate) session_id: String,
    /// 内存中的会话消息镜像，与磁盘记忆同步追加。
    pub session_messages: Vec<Message>,

    // ── 提取的子结构体 ──────────────────────────────────────
    /// LLM 模型配置、凭证与 fallback 链。
    pub(crate) model_ctx: model_ctx::ModelContext,
    /// 压缩与上下文维护状态。
    pub(crate) compression: compression_state::CompressionState,
    /// 轮次与工具深度追踪。
    pub(crate) turn: turn_budget::TurnState,

    // ── 不可拆（非 Send 或强耦合） ──────────────────────────
    pub(crate) memory: MemoryManager,
    pub(crate) sessions: Box<dyn ConversationStore>,
    pub(crate) compression_policy: Box<dyn crate::compression::CompressionPolicy>,
    pub(crate) tool_registry: ToolRegistry,
    pub(crate) mcp_hub: Arc<TokioMutex<McpHub>>,

    // ── 注入的依赖 ─────────────────────────────────────────
    /// 进程内插件钩子总线（Block / Modify / Inject）。
    pub(crate) hook_bus: Arc<::hooks::PluginHookBus>,
    /// First-class subagent thread dispatcher.
    pub(crate) execution: Arc<dyn tools::AgentThreadDispatch>,

    // ── 轻量状态 ───────────────────────────────────────────
    pub(crate) cancel: CancelSignal,
    /// 代码/项目根（委派 worktree 或会话级 ASTRO_PROJECT_ROOT）。
    pub(crate) project_root: Option<PathBuf>,
    /// Per-session permission profile override. `None` inherits current workspace selection.
    pub(crate) permission_profile: Option<String>,
    /// `pre_llm_call` 注入的本轮附加上下文（不回写用户原文）。
    pub(crate) pending_inject_context: Option<String>,
    /// 上一轮复杂任务后挂起的学习 nudge（本轮注入 dynamic，下一次 begin_user_turn 清掉/重算）。
    pub(crate) pending_learning_nudge: Option<String>,
    /// 当前聊天交互模式（Plan/Ask 只读门禁）；由 ChatRequest 下传。
    pub(crate) interaction_mode: types::InteractionMode,
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
        let sessions: Box<dyn ConversationStore> = Box::new(SessionStore::open_sessions_dir(
            &config.memory_dir.join("sessions"),
        )?);
        let session_messages = hydrate_session_messages(&*sessions, &session_id)?;
        let mut tool_registry = ToolRegistry::new();
        register_all(&mut tool_registry);
        tool_registry.reload_enabled_from_disk(Some(&agent_id));
        let mut mcp_hub_inner = McpHub::new();
        mcp_hub_inner.set_agent_id(Some(agent_id));
        let mcp_hub = Arc::new(TokioMutex::new(mcp_hub_inner));

        let execution: Arc<dyn tools::AgentThreadDispatch> =
            Arc::new(crate::exec::dispatch::DefaultAgentThreadDispatch);

        static RECOVER_THREADS_ONCE: std::sync::Once = std::sync::Once::new();
        RECOVER_THREADS_ONCE.call_once(|| {
            if let Ok(store) = subagents::AgentThreadStore::open_default() {
                if let Err(error) = store.interrupt_stale_running() {
                    tracing::warn!(%error, "failed to recover stale subagent threads");
                }
            }
        });

        let compression_cfg = memory::load_compression_config(&config.memory_dir);
        let compression_policy: Box<dyn crate::compression::CompressionPolicy> = Box::new(
            crate::compression::StagedCompressionPolicy::from_config(&compression_cfg),
        );

        Ok(AgentLoop {
            config,
            session_id,
            session_messages,
            model_ctx: model_ctx::ModelContext::default(),
            compression: compression_state::CompressionState::default(),
            turn: turn_budget::TurnState::default(),
            memory,
            sessions,
            compression_policy,
            tool_registry,
            mcp_hub,
            hook_bus: Arc::new(::hooks::PluginHookBus::new()),
            execution,
            cancel: CancelSignal::new(),
            project_root: resolve_session_project_root(),
            permission_profile: None,
            pending_inject_context: None,
            pending_learning_nudge: None,
            interaction_mode: types::InteractionMode::Agent,
        })
    }

    /// 绑定当前流式 run 的 turn_id（约定与 `run_id` 相同）。
    pub fn set_current_turn_id(&mut self, turn_id: impl Into<String>) {
        self.turn.set_current_turn_id(turn_id);
    }

    /// 清除当前 turn_id（run 结束或中断时调用）。
    pub fn clear_current_turn_id(&mut self) {
        self.turn.clear_current_turn_id();
    }

    /// 当前绑定的 turn_id（若有）。
    pub fn current_turn_id(&self) -> Option<&str> {
        self.turn.current_turn_id()
    }

    /// 子 Agent 执行调度器。
    pub fn execution(&self) -> Arc<dyn tools::AgentThreadDispatch> {
        Arc::clone(&self.execution)
    }

    pub fn set_permission_profile(&mut self, profile: Option<String>) {
        self.permission_profile = profile;
    }

    pub fn permission_profile(&self) -> Option<&str> {
        self.permission_profile.as_deref()
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
    pub fn fire_hook(&self, name: &str, payload: ::hooks::HookPayload) -> ::hooks::HookOutcome {
        self.hook_bus.fire(name, &payload)
    }

    /// 取出并清空本轮 `pre_llm_call` 注入上下文。
    pub fn take_inject_context(&mut self) -> Option<String> {
        self.pending_inject_context.take()
    }

    /// 排队下一轮注入上下文（复用 `pre_llm_call` 的注入机制）。
    ///
    /// 供 `pre_verify` 的 `KeepGoing(msg)` 等下游控制流场景使用：不回写
    /// `session_messages`，仅在下一轮构建 API history 时以 `[astro:hook-context]`
    /// 形式追加一条 user 消息。
    pub fn queue_inject_context(&mut self, ctx: impl Into<String>) {
        self.pending_inject_context = Some(ctx.into());
    }

    /// 当前会话轮次序号（从 1 起，未开始为 0）。
    pub fn session_turn(&self) -> usize {
        self.turn.current_turn()
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

    /// 覆盖采样温度（OpenRouter default_parameters 等）。
    pub fn set_temperature(&mut self, temperature: f32) {
        self.config.temperature = temperature;
    }

    /// 透传给 Provider 的额外 JSON 参数引用。
    pub fn additional_params(&self) -> &Value {
        &self.config.additional_params
    }

    /// 覆盖 Provider 扩展参数（与已有 map 合并由调用方决定）。
    pub fn set_additional_params(&mut self, params: Value) {
        self.config.additional_params = params;
    }

    /// 当前用户消息的工具深度是否已达 `multi_turn` 上限。
    pub fn is_tool_depth_exhausted(&self) -> bool {
        self.turn.is_tool_depth_exhausted(self.config.multi_turn)
    }

    /// 热读 `compression:` 段（与 learning 同类）。
    pub fn compression_config(&self) -> memory::CompressionConfig {
        memory::load_compression_config(self.memory_dir())
    }

    pub fn config_protect_last_n(&self) -> usize {
        self.compression_config().protect_last_n.max(1)
    }

    pub fn config_protect_first_n(&self) -> usize {
        self.compression_config().protect_first_messages.max(1)
    }

    pub fn mid_run_summary_done(&self) -> bool {
        self.compression.mid_run_summary_done()
    }

    pub fn mid_run_handoff(&self) -> Option<&str> {
        self.compression.mid_run_handoff()
    }

    pub fn set_mid_run_handoff(&mut self, text: String) {
        self.compression.set_mid_run_handoff(text);
    }

    pub fn mark_mid_run_summary_skipped(&mut self) {
        self.compression.mark_mid_run_summary_skipped();
    }

    pub fn should_recommend_compact(&self) -> bool {
        self.compression.should_recommend_compact()
    }

    pub fn take_recommend_compact(&mut self) -> bool {
        self.compression.take_recommend_compact()
    }

    /// 本轮用户消息内是否已发生磁盘写入（`terminal` / `file_ops` 写类操作）。
    pub fn turn_wrote_disk(&self) -> bool {
        self.turn.turn_wrote_disk()
    }

    /// 递增工具轮次计数；超出 `multi_turn` 时返回 [`MaxDepthError`]。
    pub fn increment_tool_round(&mut self) -> Result<(), MaxDepthError> {
        self.turn.increment_tool_round(self.config.multi_turn)
    }

    /// 设置图像生成工具的输出目标路径。
    pub fn set_image_gen_targets(&mut self, targets: types::ImageGenTargets) {
        self.model_ctx.set_image_gen_targets(targets);
    }

    /// 配置 LLM 对话凭据，供需要调用 Provider 的内置工具使用。
    pub fn set_chat_credentials(
        &mut self,
        provider: &str,
        model: &str,
        api_key: &str,
        base_url: &str,
    ) {
        self.model_ctx
            .set_credentials(provider, model, api_key, base_url);
    }

    /// Agno 风格主模型入口：`Agent(model=…)`。
    ///
    /// 更新 `chat_provider` / `chat_model` 与可选温度；若已有 `chat_targets`，
    /// 用本规格覆盖 primary 的 provider/model（保留 api_key / base_url）。
    pub fn set_model(&mut self, spec: types::ModelSpec) {
        if !spec.provider_id.trim().is_empty() {
            self.model_ctx.credentials.provider = spec.provider_id.trim().to_string();
        }
        if !spec.model_id.trim().is_empty() {
            self.model_ctx.credentials.model = spec.model_id.trim().to_string();
        }
        if let Some(t) = spec.temperature {
            self.config.temperature = t;
        }
        if let Some(primary) = self.model_ctx.chat_targets.first_mut() {
            *primary = spec.apply_to(primary);
            self.model_ctx.credentials.api_key = primary.api_key.clone();
            self.model_ctx.credentials.base_url = primary.base_url.clone();
        } else if !self.model_ctx.credentials.api_key.is_empty()
            || !self.model_ctx.credentials.base_url.is_empty()
        {
            let target = spec.to_chat_target(
                &self.model_ctx.credentials.api_key,
                &self.model_ctx.credentials.base_url,
            );
            self.model_ctx.chat_targets = vec![target];
        }
        self.model_ctx.model_spec = Some(spec);
    }

    /// 按角色设置模型（主聊或辅助任务）。
    pub fn set_role_model(&mut self, role: types::ModelRole, spec: types::ModelSpec) {
        match role {
            types::ModelRole::Main => self.set_model(spec),
            types::ModelRole::Auxiliary(task) => {
                let base = self.primary_chat_target();
                let target = spec.apply_to(&base);
                self.model_ctx.auxiliary_targets.insert(task, vec![target]);
            }
        }
    }

    /// Agno 风格主聊 fallback 入口：只改 `chat_targets[1..]`，保留 primary。
    ///
    /// - 条数上限：[`types::MAX_CHAT_FALLBACKS`]
    /// - 凭据：默认继承 primary 的 `api_key` / `base_url`（跨厂商且 key 不同时，
    ///   请改用已解析的 [`Self::set_chat_targets`]）
    /// - 同 `provider_id` 去重（对齐 `expand_chat_targets`）
    pub fn set_fallback_models(&mut self, specs: &[types::ModelSpec]) {
        self.model_ctx.set_fallback_models(specs);
    }

    /// 按角色设置 fallback 链（主聊或辅助任务）。
    ///
    /// 辅助任务：保留 preferred（链首；若尚无则先用当前主目标），再接 fallback。
    pub fn set_role_fallback_models(&mut self, role: types::ModelRole, specs: &[types::ModelSpec]) {
        match role {
            types::ModelRole::Main => self.set_fallback_models(specs),
            types::ModelRole::Auxiliary(task) => {
                let preferred = self
                    .model_ctx
                    .auxiliary_targets
                    .get(&task)
                    .and_then(|v| v.first())
                    .cloned()
                    .unwrap_or_else(|| self.primary_chat_target());
                let mut chain = vec![preferred.clone()];
                let mut seen = std::collections::HashSet::new();
                seen.insert(preferred.provider_id.clone());
                for spec in specs.iter().take(types::MAX_CHAT_FALLBACKS * 2) {
                    if chain.len() > types::MAX_CHAT_FALLBACKS {
                        break;
                    }
                    let t = spec.apply_to(&preferred);
                    if t.provider_id.trim().is_empty() || !seen.insert(t.provider_id.clone()) {
                        continue;
                    }
                    chain.push(t);
                }
                self.model_ctx.auxiliary_targets.insert(task, chain);
            }
        }
    }

    /// 当前主模型声明（若有）。
    pub fn model_spec(&self) -> Option<&types::ModelSpec> {
        self.model_ctx.model_spec()
    }

    fn primary_chat_target(&self) -> types::ChatTarget {
        self.model_ctx.primary_chat_target()
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
    pub fn set_chat_targets(&mut self, targets: Vec<types::ChatTarget>) {
        self.model_ctx.set_chat_targets(targets);
    }

    /// 当前聊天 fallback 链。
    pub fn chat_targets(&self) -> &[types::ChatTarget] {
        self.model_ctx.chat_targets()
    }

    /// 设置五类辅助任务的已解析目标链（每次 `Chat` 请求由 backend 下传后调用）。
    ///
    /// 调用方保证不落盘：本方法只存内存，session 结束或进程重启即丢弃。
    pub fn set_auxiliary_targets(
        &mut self,
        targets: std::collections::HashMap<types::AuxiliaryTask, Vec<types::ChatTarget>>,
    ) {
        self.model_ctx.set_auxiliary_targets(targets);
    }

    /// 返回指定辅助任务的目标链（preferred + 可选 fallback）。
    ///
    /// 未传输该任务目标时回退当前主 `ChatTarget`（`chat_targets` 的首项，缺失时
    /// 由 `set_chat_credentials` 字段现造一条），保持旧客户端兼容。
    pub fn auxiliary_targets(&self, task: types::AuxiliaryTask) -> Vec<types::ChatTarget> {
        self.model_ctx.auxiliary_targets(task)
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
    pub fn sessions(&self) -> &dyn ConversationStore {
        &*self.sessions
    }

    /// 当前 Agent 工作区路径。
    pub fn workspace_dir(&self) -> std::path::PathBuf {
        self.memory.workspace_dir.clone()
    }

    pub fn chat_api_key(&self) -> &str {
        self.model_ctx.chat_api_key()
    }

    pub fn chat_base_url(&self) -> &str {
        self.model_ctx.chat_base_url()
    }

    pub fn chat_provider(&self) -> &str {
        self.model_ctx.chat_provider()
    }

    pub fn chat_model(&self) -> &str {
        self.model_ctx.chat_model()
    }

    pub fn image_gen_targets(&self) -> &types::ImageGenTargets {
        self.model_ctx.image_gen_targets()
    }

    /// 内置与 MCP 工具的注册表只读引用。
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }

    /// 工具注册表可变引用（编排子步剔除 `orchestration_*` 等）。
    pub fn tool_registry_mut(&mut self) -> &mut ToolRegistry {
        &mut self.tool_registry
    }

    /// MCP Hub 共享句柄，用于外部查询或调试。
    pub fn mcp_hub(&self) -> Arc<TokioMutex<McpHub>> {
        Arc::clone(&self.mcp_hub)
    }

    /// 从磁盘重载当前 Agent 的工具启用开关（gate 配置）。
    pub fn reload_tool_gates(&mut self) {
        let agent_id = self.memory.agent_id.clone();
        self.tool_registry.reload_enabled_from_disk(Some(&agent_id));
    }

    /// 从磁盘重载 MCP 配置，并将启用工具挂接到 [`ToolRegistry`]。
    ///
    /// 失败时仅记录 warn 日志，不中断调用方；成功后 MCP 工具以 `MCP_TOOLSET` 注册。
    pub async fn reload_mcp(&mut self) {
        let agent_id = self.memory.agent_id.clone();
        {
            let mut hub = self.mcp_hub.lock().await;
            if let Err(e) = hub.reload_from_disk(Some(&agent_id)).await {
                tracing::warn!(error = %e, "reload MCP failed");
            }
        }
        self.attach_mcp_tools().await;
    }

    /// 同时重载工具 gate 与 MCP 配置，通常在每轮用户输入开始时调用。
    pub async fn reload_tools_and_mcp(&mut self) {
        self.reload_tool_gates();
        self.reload_mcp().await;
    }

    /// 将 MCP Hub 中已启用的工具条目同步到 [`ToolRegistry`]。
    ///
    /// 先卸载旧 `MCP_TOOLSET` 再逐条注册，保证与磁盘 enablement 一致。
    async fn attach_mcp_tools(&mut self) {
        let entries = self.mcp_hub.lock().await.enabled_tool_entries();
        self.tool_registry.unregister_toolset(MCP_TOOLSET);
        for spec in entries {
            self.tool_registry.register(ToolEntry {
                name: spec.qualified_name,
                toolset: MCP_TOOLSET.to_string(),
                description: spec.description,
                schema: spec.schema,
                check_fn: None,
                icon: "plug",
                ..ToolEntry::lifecycle_defaults()
            });
        }
    }

    /// 最近一次记忆召回的格式化文本，已注入动态上下文。
    pub fn recalled_context(&self) -> &str {
        self.compression.recalled_context()
    }

    /// 生成新的任务 UUID，供上层追踪单次 LLM 请求。
    pub fn new_task_id(&self) -> String {
        Uuid::new_v4().to_string()
    }

    /// 会话轮次预算是否已耗尽（`current_turn >= max_turns`）。
    pub fn is_budget_exhausted(&self) -> bool {
        self.turn.is_budget_exhausted(self.config.max_turns)
    }

    /// 递增会话轮次计数（每处理一条用户消息调用一次）。
    pub fn increment_turn(&mut self) {
        self.turn.increment_turn();
    }

    /// 设置主模型上下文窗口（token），供分阶段 tool 压缩使用。
    pub fn set_context_window(&mut self, window: u32) {
        self.model_ctx.set_context_window(window);
    }

    /// 设置本轮交互模式（Plan/Ask 启用只读工具门禁）。
    pub fn set_interaction_mode(&mut self, mode: types::InteractionMode) {
        self.interaction_mode = mode;
    }

    pub fn interaction_mode(&self) -> types::InteractionMode {
        self.interaction_mode
    }

    /// 按当前交互模式过滤后的工具 schema（OpenAI tools 数组）。
    pub fn schemas_for_api(&self) -> Vec<serde_json::Value> {
        tools::filter_schemas(self.interaction_mode, self.tool_registry.schemas_for_api())
    }

    pub fn context_window(&self) -> u32 {
        self.model_ctx.context_window()
    }

    /// 解析当前 Agent 工作区目录，供工具上下文注入。
    fn resolve_workspace_dir(&self) -> PathBuf {
        self.memory.workspace_dir.clone()
    }
}

// ── 其余 impl AgentLoop 方法见子模块 ──────────────────────
// context_maintenance.rs — maintain_tool_context / provider_history / occupancy_ratio
// turn_lifecycle.rs — begin_user_turn / run_turn / run_turn_with_images / prepare_llm_context
// recording.rs — record_assistant_* / record_tool_* / register_media_artifacts
// tool_dispatch.rs — dispatch_named_tool / handle_tool_call_async / finalize_tool_call_result
// system_prompt.rs — build_system_prompt / system_prompt_layer_*

// ── 以下仍在同文件的辅助 ──────────────────────────────────

/// 判断一次工具调用是否可能写入磁盘（供 `turn_wrote_disk` 标记使用）。
///
/// `terminal` 命令不受限，保守视为总是可能写盘；`file_ops` 仅在写类
/// `operation`（`write`/`append`/`delete`/`mkdir`）时视为写盘，`read`/`list` 不算。
/// 启发式判断用户消息是否像「纠正上一轮」（中英常见提示语）。

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Arc, Mutex};

    use tempfile::TempDir;

    fn test_config(dir: &TempDir) -> AgentConfig {
        AgentConfig::with_defaults(dir.path().to_path_buf())
    }

    #[tokio::test]
    async fn finalize_tool_call_result_keeps_raw_delegate_json_for_internal_control_flow() {
        let dir = TempDir::new().unwrap();
        let mut agent = AgentLoop::new(test_config(&dir)).unwrap();
        agent.set_current_turn_id("turn-1");

        let bus = agent.hook_bus();
        let post_result: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let post_result2 = Arc::clone(&post_result);
        let subagent_stop: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let subagent_stop2 = Arc::clone(&subagent_stop);

        bus.register(::hooks::TRANSFORM_TOOL_RESULT, |_| {
            ::hooks::HookOutcome::ReplaceText("REDACTED".into())
        });
        bus.register(::hooks::POST_TOOL_CALL, move |payload| {
            *post_result2.lock().unwrap() = payload.tool_result.clone();
            ::hooks::HookOutcome::Continue
        });
        bus.register(::hooks::SUBAGENT_STOP, move |payload| {
            subagent_stop2
                .lock()
                .unwrap()
                .push((payload.session_id.clone(), payload.detail.clone()));
            ::hooks::HookOutcome::Continue
        });

        let raw_result = serde_json::json!({
            "subagent": true,
            "status": "done",
            "tasks": [
                {
                    "session_id": "child-session-1",
                    "summary": "raw delegate summary"
                }
            ]
        })
        .to_string();
        let final_result = agent
            .finalize_tool_call_result(
                "subagent",
                &serde_json::json!({"ignored": true}),
                types::ToolOutput::from(raw_result),
            )
            .await;

        assert_eq!(final_result.text(), "REDACTED");
        assert_eq!(post_result.lock().unwrap().as_deref(), Some("REDACTED"));
        assert_eq!(
            subagent_stop.lock().unwrap().as_slice(),
            [(
                "child-session-1".to_string(),
                "raw delegate summary".to_string()
            )]
        );
    }

    fn t(id: &str, backend: &str, model: &str) -> types::ChatTarget {
        types::ChatTarget {
            provider_id: id.into(),
            backend_id: backend.into(),
            model: model.into(),
            api_key: format!("k-{id}"),
            base_url: format!("https://{id}.example"),
        }
    }

    #[test]
    fn auxiliary_targets_falls_back_to_chat_credentials_when_nothing_set() {
        let dir = TempDir::new().unwrap();
        let mut agent = AgentLoop::new(test_config(&dir)).unwrap();
        agent.set_chat_credentials("openai", "gpt-5.6", "key-1", "https://api.openai.com/v1");

        let targets = agent.auxiliary_targets(types::AuxiliaryTask::Dreaming);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].backend_id, "openai");
        assert_eq!(targets[0].model, "gpt-5.6");
        assert_eq!(targets[0].api_key, "key-1");
    }

    #[test]
    fn auxiliary_targets_falls_back_to_primary_chat_target_when_nothing_set() {
        let dir = TempDir::new().unwrap();
        let mut agent = AgentLoop::new(test_config(&dir)).unwrap();
        agent.set_chat_targets(vec![t("p0", "openai", "gpt-5.6")]);

        let targets = agent.auxiliary_targets(types::AuxiliaryTask::Compaction);
        assert_eq!(targets, vec![t("p0", "openai", "gpt-5.6")]);
    }

    #[test]
    fn auxiliary_targets_returns_configured_chain_for_matching_task() {
        let dir = TempDir::new().unwrap();
        let mut agent = AgentLoop::new(test_config(&dir)).unwrap();
        agent.set_chat_targets(vec![t("p0", "openai", "gpt-5.6")]);

        let mut map = std::collections::HashMap::new();
        map.insert(
            types::AuxiliaryTask::SmartApproval,
            vec![t("p1", "claude", "opus"), t("p0", "openai", "gpt-5.6")],
        );
        agent.set_auxiliary_targets(map);

        let smart = agent.auxiliary_targets(types::AuxiliaryTask::SmartApproval);
        assert_eq!(
            smart,
            vec![t("p1", "claude", "opus"), t("p0", "openai", "gpt-5.6")]
        );

        // 未配置的任务仍回退主 ChatTarget，不受其它任务配置影响。
        let dreaming = agent.auxiliary_targets(types::AuxiliaryTask::Dreaming);
        assert_eq!(dreaming, vec![t("p0", "openai", "gpt-5.6")]);
    }

    #[test]
    fn detects_user_correction_cues() {
        assert!(looks_like_user_correction("不对，应该用 rg 而不是 grep"));
        assert!(looks_like_user_correction(
            "Actually that's wrong, should be async"
        ));
        assert!(looks_like_user_correction("重来"));
        assert!(!looks_like_user_correction("帮我加一个按钮"));
        assert!(!looks_like_user_correction("继续"));
        assert!(!looks_like_user_correction(""));
    }

    #[test]
    fn learning_nudge_arms_only_when_threshold_met() {
        let dir = TempDir::new().unwrap();
        assert!(AgentLoop::compute_learning_nudge(dir.path(), 2).is_none());
        let n = AgentLoop::compute_learning_nudge(dir.path(), 5).expect("nudge");
        assert!(n.contains("skills"));
        assert!(n.contains("memory"));

        fs::write(
            dir.path().join("config.yaml"),
            "learning:\n  nudge_enabled: false\n",
        )
        .unwrap();
        assert!(AgentLoop::compute_learning_nudge(dir.path(), 9).is_none());
    }
}

/// 判断一次工具调用是否可能写入磁盘（供 `turn_wrote_disk` 标记使用）。
///
/// `terminal` 命令不受限，保守视为总是可能写盘；`file_ops` 仅在写类
/// `operation`（`write`/`append`/`delete`/`mkdir`）时视为写盘，`read`/`list` 不算。
/// 启发式判断用户消息是否像「纠正上一轮」（中英常见提示语）。
///
/// 仅作学习信号，宁缺毋滥；命中即记 DecisionLog，不改变对话流程。
fn looks_like_user_correction(msg: &str) -> bool {
    let m = msg.trim().to_lowercase();
    if m.is_empty() {
        return false;
    }
    const CUES: &[&str] = &[
        "不对",
        "错了",
        "不是这",
        "不是这样",
        "应该是",
        "应该用",
        "别这",
        "别这样",
        "不要这样",
        "重来",
        "搞错",
        "写错",
        "改一下",
        "不对吧",
        "其实是",
        "而不是",
        "actually",
        "that's wrong",
        "thats wrong",
        "not right",
        "not correct",
        "should be",
        "instead",
        "you got it wrong",
        "that's not",
        "thats not",
        "no, ",
        "incorrect",
    ];
    CUES.iter().any(|c| m.contains(c))
}

fn tool_writes_disk(name: &str, args: &Value) -> bool {
    match name {
        "terminal" => true,
        "file_ops" => matches!(
            args.get("operation")
                .and_then(|v| v.as_str())
                .map(str::to_lowercase)
                .as_deref(),
            Some("write") | Some("append") | Some("delete") | Some("mkdir") | Some("patch")
        ),
        "skills" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("load")
                .to_ascii_lowercase();
            action == "manage"
        }
        _ => false,
    }
}

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
