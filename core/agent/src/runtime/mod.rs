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

use ::session::{
    build_conversation_context, format_recalled_context, ConversationStore, NewMessage,
    SessionStore,
};
use common::message::{Message, Role};
use mcp::{call_tool_with_peer, is_mcp_tool_name, McpHub, MCP_TOOLSET};
use memory::MemoryManager;
use serde_json::Value;
use tools::{dispatch_tool, register_all, DynToolHandler, ToolContext, ToolEntry, ToolRegistry};

use crate::compression::{
    prune_tool_view,
    CompressionThrashingGuard, ContextMaintenanceResult, ToolCompressionManager,
};
use crate::prompt::context::{DynamicContext, StaticContext};
use crate::prompt::hooks::CancelSignal;
use crate::prompt::prompt_builder::PromptBuilder;
use crate::runtime::session::{hydrate_session_messages, resolve_session_project_root};

pub mod budget;
pub(crate) mod compression_state;
pub(crate) mod model_ctx;
mod session;
pub(crate) mod turn_budget;
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
    config: AgentConfig,
    session_id: String,
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
    memory: MemoryManager,
    sessions: Box<dyn ConversationStore>,
    compression_policy: Box<dyn crate::compression::CompressionPolicy>,
    tool_registry: ToolRegistry,
    mcp_hub: Arc<TokioMutex<McpHub>>,

    // ── 注入的依赖 ─────────────────────────────────────────
    /// 进程内插件钩子总线（Block / Modify / Inject）。
    hook_bus: Arc<::hooks::PluginHookBus>,
    /// 子 Agent 执行调度器（统一 delegate/orchestration）。
    execution: Arc<dyn tools::ExecutionDispatch>,

    // ── 轻量状态 ───────────────────────────────────────────
    cancel: CancelSignal,
    /// 代码/项目根（委派 worktree 或会话级 ASTRO_PROJECT_ROOT）。
    project_root: Option<PathBuf>,
    /// `pre_llm_call` 注入的本轮附加上下文（不回写用户原文）。
    pending_inject_context: Option<String>,
    /// 上一轮复杂任务后挂起的学习 nudge（本轮注入 dynamic，下一次 begin_user_turn 清掉/重算）。
    pending_learning_nudge: Option<String>,
    /// 当前聊天交互模式（Plan/Ask 只读门禁）；由 ChatRequest 下传。
    interaction_mode: tools::InteractionMode,
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
        let sessions: Box<dyn ConversationStore> = Box::new(
            SessionStore::open_sessions_dir(&config.memory_dir.join("sessions"))?,
        );
        let session_messages = hydrate_session_messages(&*sessions, &session_id)?;
        let mut tool_registry = ToolRegistry::new();
        register_all(&mut tool_registry);
        tool_registry.reload_enabled_from_disk(Some(&agent_id));
        let mut mcp_hub_inner = McpHub::new();
        mcp_hub_inner.set_agent_id(Some(agent_id));
        let mcp_hub = Arc::new(TokioMutex::new(mcp_hub_inner));

        let execution: Arc<dyn tools::ExecutionDispatch> =
            Arc::new(crate::exec::dispatch::DefaultExecutionDispatch);

        static RESUME_ONCE: std::sync::Once = std::sync::Once::new();
        let exec_resume = Arc::clone(&execution);
        RESUME_ONCE.call_once(move || {
            let exec = Arc::clone(&exec_resume);
            delegate::resume_incomplete_async_delegates(move |task_id, req| {
                exec.spawn_async(task_id, req);
            });
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                let exec = exec_resume;
                handle.spawn(async move {
                    if let Err(e) =
                        crate::exec::orchestration::resume_incomplete_orchestrations(&*exec).await
                    {
                        tracing::warn!(error = %e, "orchestration resume failed");
                    }
                });
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
            pending_inject_context: None,
            pending_learning_nudge: None,
            interaction_mode: tools::InteractionMode::Agent,
        })
    }

    /// 绑定当前流式 run 的 turn_id（约定与 `run_id` 相同）。
    pub fn set_current_turn_id(&mut self, turn_id: impl Into<String>) {
        self.turn.current_turn_id = Some(turn_id.into());
    }

    /// 清除当前 turn_id（run 结束或中断时调用）。
    pub fn clear_current_turn_id(&mut self) {
        self.turn.current_turn_id = None;
    }

    /// 当前绑定的 turn_id（若有）。
    pub fn current_turn_id(&self) -> Option<&str> {
        self.turn.current_turn_id.as_deref()
    }

    /// 子 Agent 执行调度器。
    pub fn execution(&self) -> Arc<dyn tools::ExecutionDispatch> {
        Arc::clone(&self.execution)
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
        self.turn.current_turn
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
        self.turn.tool_rounds >= self.config.multi_turn
    }

    /// 开始新的用户消息处理：重置 `tool_rounds` 与 `turn_wrote_disk`。
    ///
    /// 若上一轮工具次数达到 `learning.complex_task_tool_threshold`，为本轮挂起学习 nudge。
    pub fn begin_user_turn(&mut self) {
        let prev_rounds = self.turn.tool_rounds;
        self.turn.tool_rounds = 0;
        self.turn.turn_wrote_disk = false;
        let compression = memory::load_compression_config(&self.memory.base_dir);
        self.compression.guard = CompressionThrashingGuard::from_config(&compression);
        self.compression_policy = Box::new(
            crate::compression::StagedCompressionPolicy::from_config(&compression)
                .with_context_window(self.context_window()),
        );
        self.compression.mid_run_handoff = None;
        self.compression.mid_run_summary_done = false;
        self.compression.pending_recommend_compact = false;
        self.pending_learning_nudge =
            Self::compute_learning_nudge(&self.memory.base_dir, prev_rounds);
    }

    /// 根据上一轮工具次数与 DecisionLog 计算本轮是否注入学习提示。
    fn compute_learning_nudge(base: &std::path::Path, prev_tool_rounds: usize) -> Option<String> {
        let cfg = memory::load_learning_config(base);
        if !cfg.nudge_enabled {
            return None;
        }
        if prev_tool_rounds < cfg.complex_task_tool_threshold {
            return None;
        }
        let mut text = format!(
            "上一轮使用了 {prev_tool_rounds} 次工具（≥ {}）。若流程可复用：用 `skills` manage create 或 patch 固化；若是长期偏好/事实：用 `memory` 写入。闲置技能可用 action=curate 查看建议（勿自动删除）。",
            cfg.complex_task_tool_threshold
        );
        if let Ok(recent) = memory::list_recent_decisions(base, 8) {
            if recent
                .iter()
                .any(|e| e.kind == memory::DecisionKind::ToolFailure)
            {
                text.push_str(
                    " 近期有工具失败记录：若已找到正确路径，请用 skills manage patch 写回对应 Skill。",
                );
            }
        }
        Some(text)
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
        self.compression.mid_run_summary_done
    }

    pub fn mid_run_handoff(&self) -> Option<&str> {
        self.compression.mid_run_handoff.as_deref()
    }

    pub fn set_mid_run_handoff(&mut self, text: String) {
        self.compression.mid_run_handoff = Some(text);
        self.compression.mid_run_summary_done = true;
    }

    pub fn mark_mid_run_summary_skipped(&mut self) {
        self.compression.mid_run_summary_done = true;
    }

    pub fn should_recommend_compact(&self) -> bool {
        self.compression.pending_recommend_compact
    }

    pub fn take_recommend_compact(&mut self) -> bool {
        let v = self.compression.pending_recommend_compact;
        self.compression.pending_recommend_compact = false;
        v
    }

    /// Provider 发送用历史：若有 mid-run handoff 则折叠中间轮次。
    pub fn provider_history(&self) -> Vec<Message> {
        match self.compression.mid_run_handoff.as_deref() {
            Some(handoff) => crate::exec::mid_run_summary::collapse_history_with_handoff(
                &self.session_messages,
                handoff,
                self.config_protect_first_n(),
                self.config_protect_last_n(),
            ),
            None => self.session_messages.clone(),
        }
    }

    /// 当前会话占用比例（ceil chars/4 ÷ context_window）。
    pub fn occupancy_ratio(&self) -> f32 {
        ToolCompressionManager::from_config(&self.compression_config())
            .with_context_window(self.context_window())
            .occupancy_ratio(&self.session_messages)
    }

    /// 本轮用户消息内是否已发生磁盘写入（`terminal` / `file_ops` 写类操作）。
    pub fn turn_wrote_disk(&self) -> bool {
        self.turn.turn_wrote_disk
    }

    /// 递增工具轮次计数；超出 `multi_turn` 时返回 [`MaxDepthError`]。
    pub fn increment_tool_round(&mut self) -> Result<(), MaxDepthError> {
        if self.is_tool_depth_exhausted() {
            return Err(MaxDepthError {
                limit: self.config.multi_turn,
                used: self.turn.tool_rounds,
            });
        }
        self.turn.tool_rounds += 1;
        Ok(())
    }

    /// 设置图像生成工具的输出目标路径。
    pub fn set_image_gen_targets(&mut self, targets: tools::ImageGenTargets) {
        self.model_ctx.image_gen_targets = targets;
    }

    /// 配置 LLM 对话凭据，供需要调用 Provider 的内置工具使用。
    pub fn set_chat_credentials(
        &mut self,
        provider: &str,
        model: &str,
        api_key: &str,
        base_url: &str,
    ) {
        self.model_ctx.chat_provider = provider.to_string();
        self.model_ctx.chat_model = model.to_string();
        self.model_ctx.chat_api_key = api_key.to_string();
        self.model_ctx.chat_base_url = base_url.to_string();
        if !provider.trim().is_empty() || !model.trim().is_empty() {
            self.model_ctx.model_spec = Some(common::ModelSpec::new(provider, model));
        }
    }

    /// Agno 风格主模型入口：`Agent(model=…)`。
    ///
    /// 更新 `chat_provider` / `chat_model` 与可选温度；若已有 `chat_targets`，
    /// 用本规格覆盖 primary 的 provider/model（保留 api_key / base_url）。
    pub fn set_model(&mut self, spec: common::ModelSpec) {
        if !spec.provider_id.trim().is_empty() {
            self.model_ctx.chat_provider = spec.provider_id.trim().to_string();
        }
        if !spec.model_id.trim().is_empty() {
            self.model_ctx.chat_model = spec.model_id.trim().to_string();
        }
        if let Some(t) = spec.temperature {
            self.config.temperature = t;
        }
        if let Some(primary) = self.model_ctx.chat_targets.first_mut() {
            *primary = spec.apply_to(primary);
            self.model_ctx.chat_api_key = primary.api_key.clone();
            self.model_ctx.chat_base_url = primary.base_url.clone();
        } else if !self.model_ctx.chat_api_key.is_empty() || !self.model_ctx.chat_base_url.is_empty() {
            let target = spec.to_chat_target(&self.model_ctx.chat_api_key, &self.model_ctx.chat_base_url);
            self.model_ctx.chat_targets = vec![target];
        }
        self.model_ctx.model_spec = Some(spec);
    }

    /// 按角色设置模型（主聊或辅助任务）。
    pub fn set_role_model(&mut self, role: common::ModelRole, spec: common::ModelSpec) {
        match role {
            common::ModelRole::Main => self.set_model(spec),
            common::ModelRole::Auxiliary(task) => {
                let base = self.primary_chat_target();
                let target = spec.apply_to(&base);
                self.model_ctx.auxiliary_targets.insert(task, vec![target]);
            }
        }
    }

    /// Agno 风格主聊 fallback 入口：只改 `chat_targets[1..]`，保留 primary。
    ///
    /// - 条数上限：[`common::MAX_CHAT_FALLBACKS`]
    /// - 凭据：默认继承 primary 的 `api_key` / `base_url`（跨厂商且 key 不同时，
    ///   请改用已解析的 [`Self::set_chat_targets`]）
    /// - 同 `provider_id` 去重（对齐 `expand_chat_targets`）
    pub fn set_fallback_models(&mut self, specs: &[common::ModelSpec]) {
        let primary = self.primary_chat_target();
        let mut chain = vec![primary.clone()];
        let mut seen = std::collections::HashSet::new();
        seen.insert(primary.provider_id.clone());
        for spec in specs.iter().take(common::MAX_CHAT_FALLBACKS * 2) {
            if chain.len() > common::MAX_CHAT_FALLBACKS {
                break;
            }
            let t = spec.apply_to(&primary);
            if t.provider_id.trim().is_empty() || !seen.insert(t.provider_id.clone()) {
                continue;
            }
            chain.push(t);
        }
        self.model_ctx.chat_targets = chain;
    }

    /// 按角色设置 fallback 链（主聊或辅助任务）。
    ///
    /// 辅助任务：保留 preferred（链首；若尚无则先用当前主目标），再接 fallback。
    pub fn set_role_fallback_models(
        &mut self,
        role: common::ModelRole,
        specs: &[common::ModelSpec],
    ) {
        match role {
            common::ModelRole::Main => self.set_fallback_models(specs),
            common::ModelRole::Auxiliary(task) => {
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
                for spec in specs.iter().take(common::MAX_CHAT_FALLBACKS * 2) {
                    if chain.len() > common::MAX_CHAT_FALLBACKS {
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
    pub fn model_spec(&self) -> Option<&common::ModelSpec> {
        self.model_ctx.model_spec.as_ref()
    }

    fn primary_chat_target(&self) -> common::ChatTarget {
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
    pub fn set_chat_targets(&mut self, targets: Vec<common::ChatTarget>) {
        if let Some(primary) = targets.first() {
            self.model_ctx.chat_provider = primary.backend_id.clone();
            self.model_ctx.chat_model = primary.model.clone();
            self.model_ctx.chat_api_key = primary.api_key.clone();
            self.model_ctx.chat_base_url = primary.base_url.clone();
            self.model_ctx.model_spec = Some(common::ModelSpec::new(&primary.backend_id, &primary.model));
        }
        self.model_ctx.chat_targets = targets;
    }

    /// 当前聊天 fallback 链。
    pub fn chat_targets(&self) -> &[common::ChatTarget] {
        &self.model_ctx.chat_targets
    }

    /// 设置五类辅助任务的已解析目标链（每次 `Chat` 请求由 backend 下传后调用）。
    ///
    /// 调用方保证不落盘：本方法只存内存，session 结束或进程重启即丢弃。
    pub fn set_auxiliary_targets(
        &mut self,
        targets: std::collections::HashMap<common::AuxiliaryTask, Vec<common::ChatTarget>>,
    ) {
        self.model_ctx.auxiliary_targets = targets;
    }

    /// 返回指定辅助任务的目标链（preferred + 可选 fallback）。
    ///
    /// 未传输该任务目标时回退当前主 `ChatTarget`（`chat_targets` 的首项，缺失时
    /// 由 `set_chat_credentials` 字段现造一条），保持旧客户端兼容。
    pub fn auxiliary_targets(&self, task: common::AuxiliaryTask) -> Vec<common::ChatTarget> {
        if let Some(targets) = self.model_ctx.auxiliary_targets.get(&task) {
            if !targets.is_empty() {
                return targets.clone();
            }
        }
        match self.model_ctx.chat_targets.first() {
            Some(primary) => vec![primary.clone()],
            None => vec![common::ChatTarget {
                provider_id: String::new(),
                backend_id: self.model_ctx.chat_provider.clone(),
                model: self.model_ctx.chat_model.clone(),
                api_key: self.model_ctx.chat_api_key.clone(),
                base_url: self.model_ctx.chat_base_url.clone(),
            }],
        }
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
        &self.model_ctx.chat_api_key
    }

    pub fn chat_base_url(&self) -> &str {
        &self.model_ctx.chat_base_url
    }

    pub fn chat_provider(&self) -> &str {
        &self.model_ctx.chat_provider
    }

    pub fn chat_model(&self) -> &str {
        &self.model_ctx.chat_model
    }

    pub fn image_gen_targets(&self) -> &tools::ImageGenTargets {
        &self.model_ctx.image_gen_targets
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
        &self.compression.last_recalled_context
    }

    /// 生成新的任务 UUID，供上层追踪单次 LLM 请求。
    pub fn new_task_id(&self) -> String {
        Uuid::new_v4().to_string()
    }

    /// 会话轮次预算是否已耗尽（`current_turn >= max_turns`）。
    pub fn is_budget_exhausted(&self) -> bool {
        self.turn.current_turn >= self.config.max_turns
    }

    /// 递增会话轮次计数（每处理一条用户消息调用一次）。
    pub fn increment_turn(&mut self) {
        self.turn.current_turn += 1;
    }

    /// 设置主模型上下文窗口（token），供分阶段 tool 压缩使用。
    pub fn set_context_window(&mut self, window: u32) {
        self.model_ctx.context_window = if window == 0 {
            crate::prompt::context_usage::DEFAULT_CONTEXT_WINDOW
        } else {
            window
        };
    }

    /// 设置本轮交互模式（Plan/Ask 启用只读工具门禁）。
    pub fn set_interaction_mode(&mut self, mode: tools::InteractionMode) {
        self.interaction_mode = mode;
    }

    pub fn interaction_mode(&self) -> tools::InteractionMode {
        self.interaction_mode
    }

    /// 按当前交互模式过滤后的工具 schema（OpenAI tools 数组）。
    pub fn schemas_for_api(&self) -> Vec<serde_json::Value> {
        tools::filter_schemas(self.interaction_mode, self.tool_registry.schemas_for_api())
    }

    pub fn context_window(&self) -> u32 {
        if self.model_ctx.context_window == 0 {
            crate::prompt::context_usage::DEFAULT_CONTEXT_WINDOW
        } else {
            self.model_ctx.context_window
        }
    }

    /// Run 内 tool 上下文维护：委托 [`CompressionPolicy`] 生成计划，执行 prune/LLM 摘要/head-tail。
    ///
    /// 不变量：`content` 全文保留；仅改 `compressed_content`（Provider 视图）。
    pub async fn maintain_tool_context(&mut self) -> anyhow::Result<ContextMaintenanceResult> {
        let mut result = ContextMaintenanceResult::default();
        if !self.compression_config().enabled {
            return Ok(result);
        }
        if !self.compression.guard.allow_run() {
            result.thrashing_disabled = true;
            return Ok(result);
        }

        let stored = self.sessions.get_messages(&self.session_id)?;
        let protect_last_n = self.compression_config().protect_last_n.max(1);

        let plan = self.compression_policy.plan(
            &stored,
            &self.session_messages,
            self.memory_dir(),
            &self.session_id,
            protect_last_n,
        );

        if plan.prune.is_empty() && plan.compress.is_empty() {
            return Ok(result);
        }

        result.stage_ratio = plan.stage_ratio;
        result.occupancy_before = plan.occupancy_before;

        // ── Prune 阶段（不含 await，复用已有 stored 快照） ──
        for target in &plan.prune {
            let view = prune_tool_view(target.tool_name.as_deref(), target.spill_rel.as_deref());
            let Some(stored_msg) = stored.iter().find(|m| m.id == target.message_id) else {
                continue;
            };
            let content = stored_msg.content.as_deref().unwrap_or_default();
            self.apply_tool_compressed_view(stored_msg, content, &view)?;
            result.pruned += 1;
        }

        // ── Compress 阶段：先尝试 LLM 摘要，失败回退 head/tail ──
        let targets = self.auxiliary_targets(common::AuxiliaryTask::Compaction);
        let llm_budget = crate::exec::tool_llm_compress::MAX_LLM_TOOL_COMPRESS_PER_PASS;

        for (i, job) in plan.compress.iter().enumerate() {
            let view = if i < llm_budget && !targets.is_empty() {
                match crate::exec::tool_llm_compress::summarize_tool_result(
                    &targets,
                    job.tool_name.as_deref(),
                    &job.content,
                    job.max_chars,
                )
                .await
                {
                    Ok(v) => {
                        result.llm_summarized += 1;
                        v
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            tool = ?job.tool_name,
                            "tool LLM compress failed; falling back to head/tail"
                        );
                        self.compression_policy
                            .compress_fallback(job.tool_name.as_deref(), &job.content, job)
                            .unwrap_or_else(|| job.content.clone())
                    }
                }
            } else {
                self.compression_policy
                    .compress_fallback(job.tool_name.as_deref(), &job.content, job)
                    .unwrap_or_else(|| job.content.clone())
            };

            let stored_again = self.sessions.get_messages(&self.session_id)?;
            let Some(stored_msg) = stored_again.iter().find(|m| m.id == job.message_id) else {
                continue;
            };
            self.apply_tool_compressed_view(stored_msg, &job.content, &view)?;
            result.compressed += 1;
        }

        // ── 防抖 + compact 建议 ──
        let mgr = ToolCompressionManager::from_config(&self.compression_config())
            .with_context_window(self.context_window());
        result.occupancy_after = mgr.occupancy_ratio(&self.session_messages);
        self.compression
            .guard
            .record_outcome(result.occupancy_before, result.occupancy_after);
        result.thrashing_disabled = self.compression.guard.disabled;
        result.recommend_session_compact =
            self.compression_policy.should_recommend_compact(result.occupancy_after);
        if result.recommend_session_compact {
            self.compression.pending_recommend_compact = true;
        }
        Ok(result)
    }

    fn apply_tool_compressed_view(
        &mut self,
        stored_msg: &::session::StoredMessage,
        content: &str,
        view: &str,
    ) -> anyhow::Result<()> {
        self.sessions
            .update_message_compressed_content(stored_msg.id, Some(view))?;
        if let Some(runtime_msg) = self.session_messages.iter_mut().find(|m| {
            m.role == Role::Tool
                && match (&m.tool_call_id, &stored_msg.tool_call_id) {
                    (Some(a), Some(b)) => a == b,
                    (None, None) => m.content_str() == content,
                    _ => false,
                }
        }) {
            runtime_msg.compressed_content = Some(view.to_string());
        }
        Ok(())
    }

    /// 压缩本 run 中尚未压缩的 tool 结果（兼容旧调用；委托 [`Self::maintain_tool_context`]）。
    pub async fn compress_tool_results_if_needed(&mut self) -> anyhow::Result<usize> {
        let report = self.maintain_tool_context().await?;
        Ok(report.pruned + report.compressed)
    }

    /// 与 `build_system_prompt` 同源加载静态/动态上下文与技能列表（不含 env 副作用）。
    fn system_prompt_parts(&self) -> (StaticContext, DynamicContext, Vec<(String, String)>) {
        let (project_memory, user_profile, daily) = self.memory.prompt_snapshot_with_daily();
        let skill_pairs = if self.tool_registry.is_toolset_enabled("skills") {
            skills::list_enabled_for_prompt()
        } else {
            Vec::new()
        };

        let mut static_ctx = if let Some(ref over) = self.config.static_override {
            over.clone()
        } else {
            StaticContext::from_workspace_files(
                &self.config.soul,
                &project_memory,
                &user_profile,
                &daily,
            )
        };
        if static_ctx.agent_md.is_empty() {
            let ws = self.resolve_workspace_dir();
            if let Ok(content) = std::fs::read_to_string(ws.join("AGENTS.md")) {
                static_ctx.agent_md = content;
            }
        }
        let dynamic_ctx = {
            let mut dyn_ctx = DynamicContext::from_recalled(
                self.config.dynamic_max_items,
                &self.compression.last_recalled_context,
            );
            let pinned = tools::render_pinned_for_prompt(&self.memory.workspace_dir);
            if !pinned.trim().is_empty() {
                // 固定上下文优先于本轮 FTS 召回
                dyn_ctx.items.insert(0, pinned);
            }
            if let Some(ref nudge) = self.pending_learning_nudge {
                dyn_ctx.items.insert(0, format!("# 学习提示\n{nudge}"));
            }
            dyn_ctx
        };
        (static_ctx, dynamic_ctx, skill_pairs)
    }

    /// 组装完整 system prompt：静态上下文 + 动态召回 + 技能索引 + 工具指引 + 时间戳。
    ///
    /// MEMORY / USER 仅注入 **snapshot**（同会话冻结）；日记读盘后截断注入。
    /// 各层经 [`crate::prompt::ContextSource`] 共享字符预算（优先 static）。
    ///
    /// **不**把 `pending_inject_context` 编入 system：hooks / KeepGoing 注入仍走
    /// [`Self::take_inject_context`] → 消息侧 `[astro:hook-context]`（见 `multi_turn`），
    /// 避免与 system 层双重注入。
    ///
    /// 副作用：设置 `ASTRO_WORKSPACE` 环境变量供工具读取。
    pub fn build_system_prompt(&self) -> String {
        let (static_ctx, dynamic_ctx, skill_pairs) = self.system_prompt_parts();
        // SAFETY: env var set before spawning child processes; single-threaded at this call site
        unsafe { std::env::set_var("ASTRO_WORKSPACE", &self.memory.workspace_dir) };
        let skill_index: Vec<(&str, &str)> = skill_pairs
            .iter()
            .map(|(name, desc)| (name.as_str(), desc.as_str()))
            .collect();

        let (guidance, timestamp) = self.system_prompt_guidance_timestamp();
        let mut budget = crate::prompt::ContextBudget::new(self.config.context_budget_chars.max(1));
        crate::prompt::assemble_system_layers(
            &mut budget,
            &static_ctx,
            None, // inject 走 take_inject_context / user 消息，不进 system
            &skill_index,
            &dynamic_ctx,
            &guidance,
            &timestamp,
        )
    }

    /// 与 `build_system_prompt` 同源的分层字符数，供上下文占用估算。
    /// 返回 (system, memory, skills, recall)。
    pub fn system_prompt_layer_chars(&self) -> (usize, usize, usize, usize) {
        let layers = self.system_prompt_layer_breakdown();
        (
            layers.system_chars,
            layers.memory_chars,
            layers.skills_chars,
            layers.recall_chars,
        )
    }

    /// 分层占用明细（含 system / memory / skills 子项），供 `context_usage` 快照。
    pub fn system_prompt_layer_breakdown(&self) -> crate::prompt::context_usage::LayerBreakdown {
        use crate::prompt::context_usage::{estimate_tokens, LayerBreakdown, NamedChars};

        let (static_ctx, dynamic_ctx, skill_pairs) = self.system_prompt_parts();
        let skill_index: Vec<(&str, &str)> = skill_pairs
            .iter()
            .map(|(name, desc)| (name.as_str(), desc.as_str()))
            .collect();

        let (guidance, timestamp) = self.system_prompt_guidance_timestamp();
        let mode_guidance = self.interaction_mode.system_guidance();
        let tool_guidance = crate::prompt::prompt_builder::TOOL_GUIDANCE;

        let mut system_items: Vec<NamedChars> = Vec::new();
        let mut push_sys = |id: &str, label: &str, content: &str| {
            let n = content.trim().len();
            if n > 0 {
                system_items.push((id.to_string(), label.to_string(), n));
            }
        };
        push_sys("soul", "SOUL.md", &static_ctx.soul);
        push_sys("identity", "身份", &static_ctx.identity);
        push_sys("agents", "AGENTS.md", &static_ctx.agent_md);
        push_sys("mode", "交互模式引导", mode_guidance);
        push_sys("tool_guidance", "工具指引", tool_guidance);
        push_sys("timestamp", "当前时间", &timestamp);

        let system_chars = static_ctx.soul.trim().len()
            + static_ctx.identity.trim().len()
            + static_ctx.agent_md.trim().len()
            + guidance.len()
            + timestamp.len();

        let mut memory_items: Vec<NamedChars> = Vec::new();
        let mut push_mem = |id: &str, label: &str, content: &str| {
            let n = content.trim().len();
            if n > 0 {
                memory_items.push((id.to_string(), label.to_string(), n));
            }
        };
        push_mem("memory", "MEMORY.md", &static_ctx.memory);
        push_mem("user", "USER.md", &static_ctx.user_profile);
        push_mem("daily", "今日记忆", &static_ctx.daily);
        let memory_chars: usize = memory_items.iter().map(|(_, _, n)| *n).sum();

        let skills_chars = PromptBuilder::new()
            .with_skills_index(&skill_index)
            .build()
            .len();
        let skill_items: Vec<NamedChars> = skill_pairs
            .iter()
            .map(|(name, desc)| {
                let line = format!("- **{}**: {}", name, desc);
                (name.clone(), name.clone(), line.len())
            })
            .filter(|(_, _, n)| estimate_tokens(*n) > 0)
            .collect();

        let recall_chars = dynamic_ctx.render().len();

        LayerBreakdown {
            system_chars,
            memory_chars,
            skills_chars,
            recall_chars,
            system_items,
            memory_items,
            skill_items,
        }
    }

    /// guidance（mode 在前，便于预算截断时保留）+ timestamp，与 `assemble_system_layers` 顺序一致。
    fn system_prompt_guidance_timestamp(&self) -> (String, String) {
        // mode 置于 TOOL_GUIDANCE 之前：guidance 层被 take_chars 截断时优先保留模式说明。
        let guidance = format!(
            "{}\n\n{}",
            self.interaction_mode.system_guidance(),
            crate::prompt::prompt_builder::TOOL_GUIDANCE,
        );
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");
        let timestamp = format!("# 当前时间\n{now}");
        (guidance, timestamp)
    }

    /// 解析当前 Agent 工作区目录，供工具上下文注入。
    fn resolve_workspace_dir(&self) -> PathBuf {
        self.memory.workspace_dir.clone()
    }

    /// 按名称分发工具调用：MCP 走 Hub，内置工具走 [`dispatch_tool`]。
    ///
    /// 调用前刷新 gate 与 MCP 注册；未启用或不存在的工具直接 bail。
    /// MCP 工具通过克隆 `Arc<TokioMutex<McpHub>>` 构造动态 handler，
    /// 避免 `&self.mcp_hub` 与 `&mut self.memory` 的借用冲突。
    async fn dispatch_named_tool(
        &mut self,
        name: &str,
        args: &serde_json::Value,
    ) -> anyhow::Result<common::ToolOutput> {
        let agent_id = self.memory.agent_id.clone();
        self.tool_registry.reload_enabled_from_disk(Some(&agent_id));

        // MCP 工具：同步 enablement + 刷新注册
        if is_mcp_tool_name(name) {
            let _ = self.mcp_hub.lock().await.sync_enablement_from_disk();
            self.attach_mcp_tools().await;
        }

        let allowed = self.tool_registry.is_tool_allowed(name);

        // 在构造 ToolContext 之前，从 Hub 解析 peer（lock → resolve → release）
        // 构建 MCP 动态 handler，持有 Peer（Send + Sync），无需跨 await 持锁。
        let mcp_handler: Option<DynToolHandler> = if is_mcp_tool_name(name) {
            let (peer, native, timeout_secs) = self
                .mcp_hub
                .lock()
                .await
                .resolve_tool_peer(name)?;
            let qname = name.to_string();
            Some(Box::new(move |_name: &str, args: &serde_json::Value| {
                let peer = peer.clone();
                let qname = qname.clone();
                let native = native.clone();
                let a = args.clone();
                Box::pin(async move {
                    call_tool_with_peer(&peer, &qname, &native, &a, timeout_secs).await
                }) as std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<common::ToolOutput>> + Send>>
            }))
        } else {
            None
        };

        let workspace_dir = self.resolve_workspace_dir();
        // SAFETY: env var set before spawning child processes; single-threaded at this call site
        unsafe { std::env::set_var("ASTRO_WORKSPACE", &workspace_dir) };
        let image_gen_targets = self.model_ctx.image_gen_targets.clone();
        let session_id = self.session_id.clone();
        let turn_id = self.turn.current_turn_id.clone();
        let chat_api_key = self.model_ctx.chat_api_key.clone();
        let chat_base_url = self.model_ctx.chat_base_url.clone();
        let chat_provider = self.model_ctx.chat_provider.clone();
        let chat_model = self.model_ctx.chat_model.clone();
        let chat_targets = self.model_ctx.chat_targets.clone();
        let memory_dir = self.config.memory_dir.clone();
        let sessions: &dyn ConversationStore = &*self.sessions;
        let execution = Some(self.execution());
        let hook_bus = Some(self.hook_bus());
        let mut ctx = ToolContext {
            memory: &mut self.memory,
            sessions,
            memory_dir,
            workspace_dir,
            project_root: self.project_root.clone(),
            image_gen_targets: &image_gen_targets,
            session_id,
            turn_id,
            chat_api_key,
            chat_base_url,
            chat_provider,
            chat_model,
            chat_targets,
            execution,
            hook_bus,
        };
        dispatch_tool(|_| allowed, &mut ctx, name, args, mcp_handler.as_ref()).await
    }

    /// 同步执行工具调用：在无 tokio runtime 时自建 current_thread runtime。
    ///
    /// 适用于 Tauri 等同步边界；异步上下文优先使用 [`handle_tool_call_async`]。
    pub fn handle_tool_call(
        &mut self,
        name: &str,
        args: &serde_json::Value,
    ) -> anyhow::Result<common::ToolOutput> {
        let fut = self.handle_tool_call_async(name, args);
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                // 在 blocking 线程上安全执行异步任务（避免在 async 上下文中 block_on panic）
                tokio::task::block_in_place(|| handle.block_on(fut))
            }
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                rt.block_on(fut)
            }
        }
    }

    /// 异步执行单次工具调用：检查取消 → 递增深度 → hooks → 分发 → hooks。
    ///
    /// 取消或深度耗尽时返回错误；成功时返回 `ToolOutput`。
    pub async fn handle_tool_call_async(
        &mut self,
        name: &str,
        args: &serde_json::Value,
    ) -> anyhow::Result<common::ToolOutput> {
        if self.cancel.is_cancelled() {
            anyhow::bail!("prompt cancelled");
        }
        self.increment_tool_round()?;
        // 可拦截：PluginHookBus 优先
        let bus_out = self.fire_hook(
            ::hooks::PRE_TOOL_CALL,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id: self.turn.current_turn_id.clone(),
                tool_name: Some(name.into()),
                tool_args: Some(args.clone()),
                detail: format!("{name} {args}"),
                ..Default::default()
            },
        );
        let mut args_owned = args.clone();
        match bus_out {
            ::hooks::HookOutcome::Block(reason) => {
                return Ok(format!("[blocked by hook] {reason}").into());
            }
            ::hooks::HookOutcome::Modify(v) => {
                args_owned = v;
            }
            _ => {}
        }
        if self.cancel.is_cancelled() {
            anyhow::bail!("prompt cancelled");
        }
        // Soft-alias：模型把 Skill 名当工具名时，改写成 skills(skill_id=…)
        let (exec_name, exec_args) = if !is_mcp_tool_name(name)
            && !self.tool_registry.has_tool(name)
            && self.tool_registry.is_tool_allowed("skills")
            && skills::list_installed()
                .into_iter()
                .any(|s| s.name == name && s.enabled)
        {
            (
                "skills",
                serde_json::json!({
                    "action": "load",
                    "skill_id": name,
                    "input": args_owned,
                }),
            )
        } else {
            (name, args_owned)
        };
        if let Err(msg) = tools::check_tool_call(self.interaction_mode, exec_name, &exec_args) {
            return Ok(msg.into());
        }
        let raw_result = self.dispatch_named_tool(exec_name, &exec_args).await?;
        if exec_name == "skills" {
            self.activate_skill_toolsets_from_args(&exec_args);
        }
        // KeyChoice：`confirm` 是关键决策闸口，记一笔供学习闭环。
        if exec_name == "confirm" {
            memory::try_append_decision(
                self.memory.base_dir.as_path(),
                memory::DecisionEntry::new(
                    memory::DecisionKind::KeyChoice,
                    format!(
                        "confirm: {}",
                        exec_args
                            .get("prompt")
                            .or_else(|| exec_args.get("message"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .chars()
                            .take(160)
                            .collect::<String>()
                    ),
                )
                .with_tool("confirm")
                .with_session(self.session_id.clone()),
            );
        }
        if tool_writes_disk(exec_name, &exec_args) {
            self.turn.turn_wrote_disk = true;
        }
        Ok(self
            .finalize_tool_call_result(exec_name, &exec_args, raw_result)
            .await)
    }

    /// `skills` 工具成功加载后：按 frontmatter `astro_tools` additive 放宽 toolset。
    fn activate_skill_toolsets_from_args(&mut self, args: &serde_json::Value) {
        let Some(skill_id) = args
            .get("skill_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return;
        };
        // 优先复用 skills 工具刚加载过的结果，避免二次扫描 + 读盘。
        let astro_tools = match skills::recent_astro_tools(skill_id) {
            Some(tools) => tools,
            None => match skills::load_skill_by_name(skill_id) {
                Ok(loaded) => loaded.metadata.astro_tools,
                Err(_) => return,
            },
        };
        if !astro_tools.is_empty() {
            tracing::info!(
                skill = %skill_id,
                toolsets = ?astro_tools,
                "skill activated toolsets (additive)"
            );
            self.tool_registry.activate_skill_toolsets(&astro_tools);
        }
    }

    /// `pub(crate)`：供 `exec::delegate` 的 `subagent_start`/`subagent_stop` 顺序测试复用。
    pub(crate) async fn finalize_tool_call_result(
        &self,
        name: &str,
        args_owned: &serde_json::Value,
        raw_result: common::ToolOutput,
    ) -> common::ToolOutput {
        let raw_text = raw_result.text().to_string();
        let transformed = self.fire_hook(
            ::hooks::TRANSFORM_TOOL_RESULT,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id: self.turn.current_turn_id.clone(),
                tool_name: Some(name.into()),
                tool_args: Some(args_owned.clone()),
                tool_result: Some(raw_text.clone()),
                ..Default::default()
            },
        );
        let result = match transformed {
            ::hooks::HookOutcome::ReplaceText(s) => match raw_result {
                common::ToolOutput::Media { assets, .. } => {
                    common::ToolOutput::Media { text: s, assets }
                }
                _ => common::ToolOutput::from(s),
            },
            _ => raw_result,
        };
        let _ = self.fire_hook(
            ::hooks::POST_TOOL_CALL,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id: self.turn.current_turn_id.clone(),
                tool_name: Some(name.into()),
                tool_result: Some(result.text().to_string()),
                detail: {
                    let preview: String = result.text().chars().take(200).collect();
                    format!("{name} → {preview}")
                },
                ..Default::default()
            },
        );
        if name == "subagent" || name == "pipeline" {
            self.fire_subagent_stop_from_delegate_result(&raw_text)
                .await;
        }
        result
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
            let summary = t.get("summary").and_then(|s| s.as_str()).unwrap_or("");
            let _ = self.fire_hook(
                ::hooks::SUBAGENT_STOP,
                ::hooks::HookPayload {
                    session_id: child.into(),
                    turn_id: self.turn.current_turn_id.clone(),
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
        let thought_signature =
            common::message::google_thought_signature_from_details(&reasoning_details);
        self.sessions.ensure_session(&self.session_id, "tauri")?;
        self.sessions.append_message(NewMessage {
            content: Some(content),
            tool_calls: tool_calls_json,
            reasoning,
            reasoning_details: reasoning_details.clone(),
            ..NewMessage::empty(&self.session_id, "assistant")
        })?;
        let msg = match tool_calls {
            Some(calls) if !calls.is_empty() => Message::assistant_with_tools(content, calls),
            _ => Message::assistant(content),
        };
        let mut msg = msg;
        msg.reasoning = reasoning.map(str::to_string);
        msg.thought_signature = thought_signature;
        self.session_messages.push(msg);
        Ok(())
    }

    /// 工具执行后回写最近一条 assistant 的 timeline/surfaces（避免历史丢 A2UI 卡）。
    pub fn patch_last_assistant_timeline(
        &self,
        reasoning_details: serde_json::Value,
    ) -> anyhow::Result<()> {
        self.sessions
            .patch_last_assistant_reasoning_details(&self.session_id, &reasoning_details)
    }

    /// 将 user 角色消息写入记忆与会话镜像。
    ///
    /// 供 `pre_verify` 的 `KeepGoing(msg)` 等下游控制流场景使用：与 `pending_inject_context`
    /// 的临时注入不同，本方法直接落盘并写入 `session_messages`，确保下一轮 API 历史与
    /// `SessionStore` 保持一致（角色交替），避免连续 assistant 触发 Provider 400。
    pub fn record_user_message(&mut self, content: &str) -> anyhow::Result<()> {
        self.sessions.ensure_session(&self.session_id, "tauri")?;
        self.sessions.append_message(NewMessage {
            content: Some(content),
            ..NewMessage::empty(&self.session_id, "user")
        })?;
        self.session_messages.push(Message::user(content));
        Ok(())
    }

    /// 将 tool 角色结果写入记忆与会话镜像（无 tool_call_id / tool_name）。
    pub fn record_tool_result(&mut self, content: &str) -> anyhow::Result<()> {
        self.record_tool_result_with_id(None, None, content)
    }

    /// 将工具生成的媒体文件实时登记到 artifacts 索引，关联当前会话与消息。
    ///
    /// 否则这些文件仅在文件空间 `reconcile` 扫盘时以 `session_id=None` 补登记，
    /// 导致「会话中生成的文件」被归入「未关联会话」。
    fn register_media_artifacts(&self, media: &[common::MediaAsset], msg_id: i64) {
        if media.is_empty() {
            return;
        }
        let db = match artifacts::open_default(self.memory_dir()) {
            Ok(db) => db,
            Err(e) => {
                tracing::debug!(error = %e, "open artifacts db failed; skip media register");
                return;
            }
        };
        let workspace = self.memory.workspace_dir.clone();
        let session_id = self.session_id.clone();
        let message_id = msg_id.to_string();
        let agent_id = self.agent_id().to_string();
        for asset in media {
            let Some(rel) = asset.workspace_path() else {
                continue; // data URL / 远程 URI 不落盘，跳过
            };
            let abs = workspace.join(rel);
            let Some(path) = abs.to_str() else {
                continue;
            };
            if let Err(e) = db.register(
                path,
                artifacts::ArtifactSource::AgentWrite,
                Some(&session_id),
                Some(&message_id),
                Some(&agent_id),
            ) {
                tracing::debug!(error = %e, path, "register media artifact failed");
            }
        }
    }

    /// 将 tool 角色结果写入记忆与会话镜像，并关联 `tool_call_id` / `tool_name`。
    pub fn record_tool_result_with_id(
        &mut self,
        tool_call_id: Option<&str>,
        tool_name: Option<&str>,
        content: &str,
    ) -> anyhow::Result<()> {
        let (_, media) = common::extract_tool_media(content);
        let media_owned = if media.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&media)?)
        };
        self.sessions.ensure_session(&self.session_id, "tauri")?;
        let msg_id = self.sessions.append_message(NewMessage {
            content: Some(content),
            tool_call_id,
            tool_name,
            media_json: media_owned.as_deref(),
            ..NewMessage::empty(&self.session_id, "tool")
        })?;

        self.register_media_artifacts(&media, msg_id);

        let mut spill_view: Option<String> = None;
        if content.len() >= common::DEFAULT_SPILL_THRESHOLD_BYTES {
            match common::write_tool_spill(self.memory_dir(), &self.session_id, msg_id, content) {
                Ok(path) => {
                    let rel = common::spill_path_for_prompt(self.memory_dir(), &path);
                    let view = common::make_spill_view(tool_name, &rel, content.len(), content);
                    if self
                        .sessions
                        .update_message_compressed_content(msg_id, Some(&view))
                        .is_ok()
                    {
                        spill_view = Some(view);
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "tool spill write failed; keeping inline content");
                }
            }
        }

        let mut msg = match tool_call_id {
            Some(id) if !id.is_empty() => Message::tool_with_id(id, content),
            _ => Message::tool(content),
        };
        msg.media = media;
        if let Some(view) = spill_view {
            msg.compressed_content = Some(view);
        }
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
    /// FTS 仍只索引文本；附图写入 `messages.media_json` 并进入内存 `session_messages`。
    pub async fn run_turn_with_images(
        &mut self,
        user_message: &str,
        image_data_urls: &[String],
        _task_id: &str,
    ) -> anyhow::Result<TurnResult> {
        // 新一轮用户输入：清除上一轮 stop/cancel 遗留的协作取消标记。
        self.cancel.reset();
        if self.is_budget_exhausted() {
            return Ok(TurnResult::BudgetExhausted);
        }

        self.begin_user_turn();
        // UserCorrection：上一轮已有回复且本轮像是纠错 → 记一笔供学习闭环。
        if looks_like_user_correction(user_message)
            && self
                .session_messages
                .iter()
                .any(|m| matches!(m.role, common::message::Role::Assistant))
        {
            memory::try_append_decision(
                self.memory.base_dir.as_path(),
                memory::DecisionEntry::new(
                    memory::DecisionKind::UserCorrection,
                    user_message.chars().take(200).collect::<String>(),
                )
                .with_session(self.session_id.clone()),
            );
        }
        self.reload_tools_and_mcp().await;

        self.sessions.ensure_session(&self.session_id, "tauri")?;
        let media_assets: Vec<common::MediaAsset> = image_data_urls
            .iter()
            .map(|u| u.trim())
            .filter(|u| !u.is_empty())
            .map(|u| {
                let mime = u
                    .strip_prefix("data:")
                    .and_then(|rest| rest.split(';').next())
                    .unwrap_or("image/*")
                    .to_string();
                common::MediaAsset::data_url(common::MediaKind::Image, u, mime)
            })
            .collect();
        let media_owned = if media_assets.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&media_assets)?)
        };
        self.sessions.append_message(NewMessage {
            content: Some(user_message),
            media_json: media_owned.as_deref(),
            ..NewMessage::empty(&self.session_id, "user")
        })?;

        let fts_keywords = if self.turn.current_turn >= self.config.recent_turns {
            Some(user_message)
        } else {
            None
        };
        let recalled = build_conversation_context(
            &*self.sessions,
            &self.session_id,
            self.config.recent_turns,
            fts_keywords,
        )?;
        self.compression.last_recalled_context = format_recalled_context(&recalled);

        self.session_messages
            .push(Message::user_with_images(user_message, image_data_urls));
        self.increment_turn();
        let system_prompt = self.build_system_prompt();
        let _ = self.fire_hook(
            ::hooks::ON_SESSION_START,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id: self.turn.current_turn_id.clone(),
                detail: format!("session={}", self.session_id),
                ..Default::default()
            },
        );
        let inject = self.fire_hook(
            ::hooks::PRE_LLM_CALL,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id: self.turn.current_turn_id.clone(),
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
            turn: self.turn.current_turn,
            system_prompt,
        })
    }
}

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
                common::ToolOutput::from(raw_result),
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

    fn t(id: &str, backend: &str, model: &str) -> common::ChatTarget {
        common::ChatTarget {
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

        let targets = agent.auxiliary_targets(common::AuxiliaryTask::Dreaming);
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

        let targets = agent.auxiliary_targets(common::AuxiliaryTask::Compaction);
        assert_eq!(targets, vec![t("p0", "openai", "gpt-5.6")]);
    }

    #[test]
    fn auxiliary_targets_returns_configured_chain_for_matching_task() {
        let dir = TempDir::new().unwrap();
        let mut agent = AgentLoop::new(test_config(&dir)).unwrap();
        agent.set_chat_targets(vec![t("p0", "openai", "gpt-5.6")]);

        let mut map = std::collections::HashMap::new();
        map.insert(
            common::AuxiliaryTask::SmartApproval,
            vec![t("p1", "claude", "opus"), t("p0", "openai", "gpt-5.6")],
        );
        agent.set_auxiliary_targets(map);

        let smart = agent.auxiliary_targets(common::AuxiliaryTask::SmartApproval);
        assert_eq!(
            smart,
            vec![t("p1", "claude", "opus"), t("p0", "openai", "gpt-5.6")]
        );

        // 未配置的任务仍回退主 ChatTarget，不受其它任务配置影响。
        let dreaming = agent.auxiliary_targets(common::AuxiliaryTask::Dreaming);
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
