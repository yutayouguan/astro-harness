//! Agent 构建器：以链式 API 组装模型参数、静态/动态上下文与工具轮次。
//!
//! 设计对齐 Rig 的 Agent Builder：先通过 [`AgentBuilder`] 收集配置，再产出不可变的
//! [`BuiltAgentSpec`] 或直接实例化 [`AgentLoop`]。未显式设置的字段会回落到工作区默认值
//!（如 `SOUL.md`、活跃 agent id、内置温度与轮次上限）。

use std::path::PathBuf;

use home::AgentRuntimeConfig;
use serde_json::Value;

use crate::prompt::context::{DynamicContext, StaticContext};
use crate::runtime::{AgentConfig, AgentLoop};

/// 构建完成的 Agent 运行时规格，可在不立即创建循环时持有或序列化传递。
///
/// 包含推理参数、上下文槽位配置与身份标识；经 [`AgentBuilder::build`] 或
/// [`AgentBuilder::build_with_session_id`] 可进一步物化为可执行的 [`AgentLoop`]。
#[derive(Clone)]
pub struct BuiltAgentSpec {
    /// Agent 工作区根目录（含 `agents/`、`sessions/` 等子树）。
    pub memory_dir: PathBuf,
    /// 逻辑 Agent 标识，对应 `agents/{id}/` 目录名。
    pub agent_id: String,
    /// 系统人格/前置提示（通常来自 `SOUL.md` 或 Builder 显式设置）。
    pub preamble: String,
    /// 采样温度，影响回复随机性。
    pub temperature: f32,
    /// 单次会话允许的最大对话轮次（用户消息计数维度）。
    pub max_turns: usize,
    /// 单条用户消息内允许的工具调用循环深度（对齐 Rig `.multi_turn(n)`）。
    pub multi_turn: usize,
    /// 注入近期对话历史的条数上限，用于控制上下文窗口。
    pub recent_turns: usize,
    /// 透传给 Provider 的额外 JSON 参数（如 `top_p`、`presence_penalty` 等）。
    pub additional_params: Value,
    /// 静态上下文片段（soul / memory / user_profile），在构建时冻结。
    pub static_context: StaticContext,
    /// 动态上下文槽位每次最多纳入的条目数。
    pub dynamic_max_items: usize,
}

/// Rig 风格的链式 Agent 构建器。
///
/// 通过 `new` 设定工作区后，以 `agent_id`、`preamble` 等方法逐项覆盖；
/// 最终调用 [`build`](Self::build) 或 [`build_spec`](Self::build_spec) 完成组装。
/// 工具迭代默认 90（Hermes `max_iterations`），近期历史默认 10 条。
pub struct AgentBuilder {
    /// Agent 持久化数据根目录。
    memory_dir: PathBuf,
    /// 可选的显式 Agent id；缺省时在 `build_spec` 阶段读取活跃 id。
    agent_id: Option<String>,
    /// 可选的系统前置提示；缺省时从工作区 `SOUL.md` 加载。
    preamble: Option<String>,
    /// 可选采样温度；缺省 0.7。
    temperature: Option<f32>,
    /// 可选会话轮次上限；缺省 90。
    max_turns: Option<usize>,
    /// 单次用户消息内允许的工具迭代次数（对齐 Hermes `max_iterations`），默认 90。
    multi_turn: usize,
    /// 近期对话注入条数，默认 10。
    recent_turns: usize,
    /// Provider 额外参数，默认 `Null`。
    additional_params: Value,
    /// 构建时绑定的静态上下文。
    static_context: StaticContext,
    /// 动态上下文最大条目数，默认 3。
    dynamic_max_items: usize,
}

impl AgentBuilder {
    /// 创建工作区绑定的构建器，其余字段使用内置默认值。
    ///
    /// # 参数
    ///
    /// - `memory_dir`：Agent 数据根路径，后续 `build_spec` 从此处解析 id 与 `SOUL.md`。
    pub fn new(memory_dir: impl Into<PathBuf>) -> Self {
        Self {
            memory_dir: memory_dir.into(),
            agent_id: None,
            preamble: None,
            temperature: None,
            max_turns: None,
            multi_turn: crate::runtime::budget::DEFAULT_MAX_ITERATIONS,
            recent_turns: 10,
            additional_params: Value::Null,
            static_context: StaticContext::default(),
            dynamic_max_items: 3,
        }
    }

    /// 指定 Agent 逻辑 id，覆盖工作区「活跃 agent」解析结果。
    pub fn agent_id(mut self, id: impl Into<String>) -> Self {
        self.agent_id = Some(id.into());
        self
    }

    /// 设置系统前置提示（人格/角色说明），优先于 `SOUL.md` 与运行时配置中的名称推导。
    pub fn preamble(mut self, text: impl Into<String>) -> Self {
        self.preamble = Some(text.into());
        self
    }

    /// 设置 LLM 采样温度。
    pub fn temperature(mut self, t: f32) -> Self {
        self.temperature = Some(t);
        self
    }

    /// 设置单次会话最大用户轮次上限。
    pub fn max_turns(mut self, n: usize) -> Self {
        self.max_turns = Some(n);
        self
    }

    /// 设置单条用户消息内的工具循环深度（每轮用户输入最多执行多少次工具往返）。
    ///
    /// 与 Rig Agent Builder 的 `.multi_turn(n)` 语义一致；独立于 [`max_turns`](Self::max_turns)
    /// 的会话级预算。
    pub fn multi_turn(mut self, n: usize) -> Self {
        self.multi_turn = n;
        self
    }

    /// 设置注入提示词的近期对话条数上限，用于截断历史以控制 token 消耗。
    pub fn recent_turns(mut self, n: usize) -> Self {
        self.recent_turns = n;
        self
    }

    /// 设置透传给 Provider 的额外 JSON 参数对象。
    pub fn additional_params(mut self, params: Value) -> Self {
        self.additional_params = params;
        self
    }

    /// 绑定静态上下文（soul、长期记忆摘要、用户画像等），在构建时写入 `AgentConfig`。
    pub fn static_context(mut self, ctx: StaticContext) -> Self {
        self.static_context = ctx;
        self
    }

    /// 配置动态上下文槽位的最大条目数（运行时按优先级填充）。
    pub fn dynamic_context(mut self, max_items: usize) -> Self {
        self.dynamic_max_items = max_items;
        self
    }

    /// 从磁盘上的 `agents/{id}/config.json` 合并运行时配置。
    ///
    /// 会写入 `agent_id`；若 Builder 尚未设置 `temperature`、`max_turns`、`additional_params`，
    /// 则用配置文件中的值覆盖。`max_turns` 同时会同步到 `multi_turn`。
    /// 当 `preamble` 为空且配置含非空 `name` 时，自动生成「你是 {name}。」作为前置提示。
    ///
    /// # 参数
    ///
    /// - `cfg`：已解析的 [`AgentRuntimeConfig`]，通常由 memory 层从工作区加载。
    pub fn from_runtime_config(mut self, cfg: &AgentRuntimeConfig) -> Self {
        self.agent_id = Some(cfg.id.clone());
        if let Some(t) = cfg.temperature {
            self.temperature = Some(t);
        }
        if let Some(m) = cfg.max_turns {
            self.max_turns = Some(m);
            self.multi_turn = m;
        }
        if let Some(ref p) = cfg.additional_params {
            self.additional_params = p.clone();
        }
        if self.preamble.is_none() && !cfg.name.is_empty() {
            self.preamble = Some(format!("你是 {}。", cfg.name));
        }
        self
    }

    /// 解析默认值并产出不可变的 [`BuiltAgentSpec`]，不创建 [`AgentLoop`]。
    ///
    /// `agent_id` 缺省时调用 `home::active_agent_id`；`preamble` 缺省时读取
    /// `agents/{id}/SOUL.md`，读取失败则使用内置 Astro 默认文案。
    pub fn build_spec(self) -> BuiltAgentSpec {
        let agent_id = self
            .agent_id
            .unwrap_or_else(|| home::active_agent_id(&self.memory_dir));
        let soul = self.preamble.unwrap_or_else(|| {
            let ws = home::agent_workspace_dir(&self.memory_dir, &agent_id);
            std::fs::read_to_string(ws.join("SOUL.md"))
                .unwrap_or_else(|_| "你是 Astro，一个自我进化的 AI 助手".to_string())
        });
        BuiltAgentSpec {
            memory_dir: self.memory_dir,
            agent_id,
            preamble: soul,
            temperature: self.temperature.unwrap_or(0.7),
            max_turns: self.max_turns.unwrap_or(90),
            multi_turn: self.multi_turn,
            recent_turns: self.recent_turns,
            additional_params: self.additional_params,
            static_context: self.static_context,
            dynamic_max_items: self.dynamic_max_items,
        }
    }

    /// 构建可运行的 [`AgentLoop`] 与对应规格。
    pub fn build(self) -> anyhow::Result<(AgentLoop, BuiltAgentSpec)> {
        self.build_with_session_id(uuid::Uuid::new_v4().to_string())
    }

    /// 使用指定 `session_id` 构建 [`AgentLoop`] 并完成配置注入。
    pub fn build_with_session_id(
        self,
        session_id: String,
    ) -> anyhow::Result<(AgentLoop, BuiltAgentSpec)> {
        let spec = self.build_spec();
        let mut config = AgentConfig::with_defaults(spec.memory_dir.clone());
        config.soul = spec.preamble.clone();
        config.max_turns = spec.max_turns;
        config.multi_turn = spec.multi_turn;
        config.recent_turns = spec.recent_turns;
        config.temperature = spec.temperature;
        config.additional_params = spec.additional_params.clone();
        config.dynamic_max_items = spec.dynamic_max_items;
        if !spec.static_context.soul.is_empty()
            || !spec.static_context.memory.is_empty()
            || !spec.static_context.user_profile.is_empty()
        {
            config.static_override = Some(spec.static_context.clone());
        }
        let agent = AgentLoop::with_session_id(config, session_id)?;
        Ok((agent, spec))
    }
}

impl BuiltAgentSpec {
    /// 按本规格中的 `dynamic_max_items` 构造空的动态上下文槽位。
    ///
    /// 供运行时在每轮提示词组装前填入检索到的片段；容量与 Builder 配置保持一致。
    pub fn dynamic_context_slot(&self) -> DynamicContext {
        DynamicContext::new(self.dynamic_max_items)
    }
}
