# Hooks 系统详细设计

> 版本：v1.0 | 日期：2026-08-10 | 状态：草稿
> 对应需求：F-14 扩展性与可组合管线
> 上游文档：[02-agent-runtime详细设计.md](02-agent-runtime详细设计.md)（round_loop、工具执行流）

本文档定义 Agent 执行管线的通用扩展机制 -- Hooks 系统。当前 Agent 的隐私过滤（PrivacyMiddleware）、安全检查（SecurityPolicy）、可观测性埋点（SpanCollector / MetricsCollector）、预算管控（BudgetManager）等横切关注点均以硬编码方式嵌入 `round_loop`。Hooks 系统将这些散落的拦截逻辑统一为可注册、可排序、可配置的管线，实现"不修改核心代码即可扩展执行行为"。

---

## 1. Hooks 系统概述

### 1.1 动机

`AgentExecutor::round_loop()` 是 Agent 的执行主干（见 [02-agent-runtime详细设计.md](02-agent-runtime详细设计.md) 第 4 节）。随着功能增长，主干中直接硬编码了多种横切关注点：

| 现状 | 位置 | 问题 |
|------|------|------|
| `PrivacyMiddleware::apply_to_request()` | LLM 调用前 | 硬编码在 Provider 调用链 |
| `SecurityPolicy::evaluate()` | 工具执行前 | 直接嵌入 `round_loop` |
| `SpanCollector::start_span()` / `end_span()` | 各阶段首尾 | 埋点代码散落在多个位置 |
| `BudgetManager::check_before_call()` | LLM 调用前 | 与业务逻辑耦合 |
| `PromptGuard::scan_tool_output()` | 工具执行后 | 紧耦合于工具结果处理 |
| `AuditStore::log()` | 工具执行后 | 审计日志分散在多处 |

这些拦截逻辑具有共同模式：在特定执行阶段介入、可选地修改数据或中止流程、互相之间有顺序要求。Hooks 系统提取这一模式为统一框架。

### 1.2 设计灵感

| 来源 | 借鉴点 |
|------|--------|
| HTTP 中间件（Tower / Axum） | 请求/响应管线、洋葱模型 |
| Claude Code Hooks | 配置文件声明 shell 命令钩子、事件过滤 |
| React Hooks / Vue Lifecycle | 生命周期事件、组合式 API |
| Webpack Tapable | 事件注册表、优先级排序、同步/异步钩子 |

### 1.3 核心原则

1. **现有拦截器成为内置 Hooks**：PrivacyMiddleware、SecurityPolicy、ObservabilitySpan 等重构为 Hook trait 实现，不再特殊处理
2. **优先级决定执行顺序**：数值越小越先执行，系统级 Hook（0-99）始终先于用户级 Hook（300+）
3. **fail-open 与 fail-closed 可配**：单个 Hook 故障时，可选择跳过（fail-open）或中止管线（fail-closed）
4. **零开销原则**：未注册任何 Hook 的事件不产生额外开销；Hook 管线执行路径不引入动态分发以外的成本

### 1.4 模块结构

```text
crates/agent-core/src/hooks/
├── mod.rs              # 公开 API：HookEvent, Hook trait, HookResult
├── event.rs            # HookEvent 枚举 + HookContext 结构
├── registry.rs         # HookRegistry：注册/注销/查询
├── pipeline.rs         # HookPipeline：按优先级执行 Hook 链
├── builtin/
│   ├── mod.rs          # 内置 Hook 导出
│   ├── privacy.rs      # PrivacyFilterHook
│   ├── security.rs     # SecurityCheckHook
│   ├── observability.rs # ObservabilityHook
│   ├── budget.rs       # BudgetCheckHook
│   ├── audit.rs        # AuditLogHook
│   └── prompt_guard.rs # PromptGuardHook
├── config.rs           # HookConfig：从 hooks.toml 加载用户配置
└── shell_hook.rs       # ShellHook：配置式 shell 命令 Hook

crates/agent-runtime/src/hooks/
├── mod.rs
├── wasm_hook.rs        # WasmPluginHook：WASM 插件注册的 Hook
└── integration.rs      # 与 round_loop 的集成胶水层
```

---

## 2. Hook 生命周期事件

### 2.1 事件目录

Hook 事件按 Agent 执行阶段分为六大类，覆盖从 Agent 初始化到消息处理的完整生命周期。

```rust
// crates/agent-core/src/hooks/event.rs

/// Hook 事件枚举：Agent 执行管线中的所有可观测/可拦截节点
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEvent {
    // ── Agent 级 ──
    /// Agent 实例创建后、首次 round_loop 前触发
    /// 用途：初始化资源、加载配置、注册额外 Hook
    OnAgentInit,
    /// Agent 完成初始化并准备接受用户输入
    /// 用途：发送就绪通知、预热缓存
    OnAgentReady,
    /// Agent 会话结束或被取消时触发
    /// 用途：清理资源、写入最终统计
    OnAgentTerminate,

    // ── Turn 级 ──
    /// 一个完整轮次（round_loop 的单次迭代）开始
    /// 用途：初始化轮次级追踪、重置计数器
    OnTurnStart,
    /// 轮次正常结束
    /// 用途：聚合本轮指标、持久化 Span 树
    OnTurnEnd,
    /// 轮次中发生不可恢复错误
    /// 用途：记录错误上下文、触发告警
    OnTurnError,

    // ── LLM 级 ──
    /// LLM 调用发出前（可修改 messages、model 参数）
    /// 用途：隐私过滤、预算检查、上下文注入
    BeforeLlmCall,
    /// LLM 调用完成后（可审查/修改响应）
    /// 用途：响应审计、token 统计、输出过滤
    AfterLlmCall,
    /// LLM 流式输出的每个 chunk 到达时
    /// 用途：实时内容过滤、流式进度追踪
    OnLlmStreamChunk,

    // ── Tool 级 ──
    /// 工具执行前（可修改参数、阻止执行）
    /// 用途：安全检查、参数校验、速率限制
    BeforeToolExecute,
    /// 工具执行完成后（可修改输出）
    /// 用途：输出脱敏、审计记录、注入扫描
    AfterToolExecute,
    /// 工具执行失败时触发
    /// 用途：错误分类、重试决策、告警
    OnToolError,

    // ── Memory 级 ──
    /// 记忆注入到上下文前（可修改注入内容）
    /// 用途：记忆过滤、相关性再排序
    BeforeMemoryInject,
    /// 记忆检索完成后（可审查检索结果）
    /// 用途：检索质量监控、缓存命中统计
    AfterMemoryRetrieve,

    // ── Message 级 ──
    /// 收到用户消息时
    /// 用途：输入预处理、意图检测、Skill 匹配
    OnUserMessage,
    /// Agent 生成助手消息时
    /// 用途：输出后处理、合规检查
    OnAssistantMessage,
    /// 系统消息注入时
    /// 用途：系统提示审计、提示注入防御
    OnSystemMessage,
}
```

### 2.2 事件携带数据（HookContext）

每个事件通过 `HookContext` 携带该阶段的可读/可写数据。Hook 通过修改 `HookContext` 中的字段来影响后续流程。

```rust
// crates/agent-core/src/hooks/event.rs

use serde_json::Value;
use std::collections::HashMap;

/// Hook 执行上下文：携带事件数据，允许 Hook 读取和修改
pub struct HookContext {
    /// 当前触发的事件类型
    pub event: HookEvent,

    /// 会话 ID
    pub conversation_id: String,

    /// 工作区 ID
    pub workspace_id: String,

    /// Agent 深度（0 = 主 Agent，1+ = 子 Agent）
    pub agent_depth: u8,

    /// 当前轮次序号（Turn 级及以下事件可用）
    pub turn_index: Option<u32>,

    /// 事件负载：根据事件类型不同，包含不同的数据
    /// Hook 可以修改此字段来改变后续行为
    pub payload: HookPayload,

    /// 元数据：只读的上下文信息
    pub metadata: HashMap<String, Value>,

    /// 中止信号：Hook 设置后，管线立即停止执行
    abort: Option<AbortReason>,

    /// 跳过信号：设置后，跳过当前被拦截的操作（如跳过工具执行）
    skip: bool,
}

/// 事件负载：按事件类型区分的可变数据
#[derive(Debug, Clone)]
pub enum HookPayload {
    /// Agent 级事件（无额外数据）
    Empty,

    /// BeforeLlmCall：可修改的 LLM 请求参数
    LlmRequest {
        /// 消息列表（可修改：追加、删除、替换消息内容）
        messages: Vec<Message>,
        /// 模型标识（可修改：降级/升级模型）
        model: String,
        /// 温度参数
        temperature: Option<f32>,
        /// 预估输入 token 数
        estimated_input_tokens: u64,
    },

    /// AfterLlmCall：LLM 响应数据
    LlmResponse {
        /// 响应内容（可修改：过滤/替换内容）
        content: String,
        /// 工具调用列表
        tool_calls: Vec<ToolCallInfo>,
        /// Token 使用量
        usage: TokenUsageInfo,
        /// 模型标识
        model: String,
    },

    /// OnLlmStreamChunk：流式 chunk 数据
    StreamChunk {
        /// chunk 文本（可修改：实时过滤）
        delta: String,
        /// 已累计接收的 token 数
        accumulated_tokens: u64,
    },

    /// BeforeToolExecute：工具调用请求
    ToolRequest {
        /// 工具名称
        tool_name: String,
        /// 调用参数（可修改）
        arguments: Value,
        /// 工具调用 ID
        call_id: String,
        /// 工具风险等级
        risk_level: RiskLevel,
    },

    /// AfterToolExecute / OnToolError：工具执行结果
    ToolResponse {
        /// 工具名称
        tool_name: String,
        /// 原始调用参数
        arguments: Value,
        /// 执行结果（可修改：过滤/替换输出）
        result: ToolResultData,
        /// 执行耗时（毫秒）
        duration_ms: u64,
        /// 是否出错
        is_error: bool,
    },

    /// Memory 级事件
    MemoryData {
        /// 检索查询
        query: String,
        /// 检索结果 / 待注入条目（可修改）
        entries: Vec<MemoryEntryInfo>,
    },

    /// Message 级事件
    MessageData {
        /// 消息角色
        role: String,
        /// 消息内容（可修改）
        content: String,
    },

    /// Turn 级事件
    TurnData {
        /// 本轮 LLM 调用次数
        llm_call_count: u32,
        /// 本轮工具调用次数
        tool_call_count: u32,
        /// 本轮耗时（毫秒，OnTurnEnd 时可用）
        duration_ms: Option<u64>,
        /// 错误信息（OnTurnError 时可用）
        error: Option<String>,
    },
}

/// 工具调用简要信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Token 使用量信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageInfo {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub model: String,
}

/// 记忆条目简要信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntryInfo {
    pub id: String,
    pub category: String,
    pub key: String,
    pub value: String,
    pub relevance_score: f64,
}

/// 工具结果数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultData {
    pub output: Vec<ToolContent>,
    pub is_error: bool,
}

/// 中止原因
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbortReason {
    pub hook_name: String,
    pub message: String,
}

impl HookContext {
    /// 标记中止：后续 Hook 不再执行，被拦截的操作取消
    pub fn abort(&mut self, hook_name: &str, message: &str) {
        self.abort = Some(AbortReason {
            hook_name: hook_name.to_string(),
            message: message.to_string(),
        });
    }

    /// 标记跳过：被拦截的操作跳过，但后续 Hook 继续执行
    pub fn skip(&mut self) {
        self.skip = true;
    }

    /// 检查是否已被中止
    pub fn is_aborted(&self) -> bool {
        self.abort.is_some()
    }

    /// 检查是否应跳过当前操作
    pub fn should_skip(&self) -> bool {
        self.skip
    }

    /// 获取中止原因
    pub fn abort_reason(&self) -> Option<&AbortReason> {
        self.abort.as_ref()
    }

    /// 设置元数据
    pub fn set_metadata(&mut self, key: &str, value: Value) {
        self.metadata.insert(key.to_string(), value);
    }

    /// 获取元数据
    pub fn get_metadata(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }
}
```

### 2.3 事件触发时机一览

| 事件 | 触发位置 | 可修改数据 | 可中止 |
|------|---------|-----------|--------|
| `OnAgentInit` | `AgentExecutor::new()` 尾部 | 无 | 是 |
| `OnAgentReady` | 首次 `round_loop` 前 | 无 | 否 |
| `OnAgentTerminate` | `round_loop` 退出后 | 无 | 否 |
| `OnTurnStart` | `round_loop` 每次迭代开头 | 无 | 否 |
| `OnTurnEnd` | `round_loop` 迭代正常结束 | TurnData | 否 |
| `OnTurnError` | `round_loop` 捕获错误时 | TurnData | 否 |
| `BeforeLlmCall` | `provider.chat_stream()` 前 | messages, model, temperature | 是 |
| `AfterLlmCall` | LLM 流式完成后 | content, tool_calls | 否 |
| `OnLlmStreamChunk` | 每个流式 delta 到达 | delta | 否 |
| `BeforeToolExecute` | `ToolRegistry::execute()` 前 | arguments, risk_level | 是 |
| `AfterToolExecute` | 工具执行成功后 | result | 否 |
| `OnToolError` | 工具执行失败时 | error | 否 |
| `BeforeMemoryInject` | ContextBuilder 注入记忆前 | entries | 是 |
| `AfterMemoryRetrieve` | embedding 搜索完成后 | entries | 否 |
| `OnUserMessage` | 用户消息到达时 | content | 否 |
| `OnAssistantMessage` | Agent 生成响应时 | content | 否 |
| `OnSystemMessage` | 系统消息注入时 | content | 否 |

---

## 3. Hook trait 定义

### 3.1 核心 trait

```rust
// crates/agent-core/src/hooks/mod.rs

use async_trait::async_trait;

/// Hook 执行结果
#[derive(Debug, Clone)]
pub enum HookResult {
    /// 继续执行后续 Hook
    Continue,
    /// 跳过当前 Hook（不影响后续 Hook）
    Skip,
    /// 中止整个管线（后续 Hook 不再执行）
    Abort { reason: String },
    /// Hook 执行出错
    Error { message: String },
}

/// Hook trait：所有 Hook 的统一接口
///
/// 每个 Hook 声明自己关注的事件和优先级，
/// 当对应事件触发时，`execute` 方法被调用。
///
/// Hook 通过修改 `HookContext` 中的 payload 来影响执行管线。
#[async_trait]
pub trait Hook: Send + Sync + 'static {
    /// Hook 的唯一标识名称
    fn name(&self) -> &str;

    /// Hook 关注的事件列表
    /// 返回多个事件表示同一个 Hook 监听多个阶段
    fn events(&self) -> Vec<HookEvent>;

    /// 执行优先级（数值越小越先执行）
    ///
    /// 保留范围：
    /// - 0-99：系统级（可观测性、基础设施）
    /// - 100-199：安全级（权限检查、风险评估）
    /// - 200-299：隐私级（数据过滤、脱敏）
    /// - 300-399：业务级（预算、审计）
    /// - 400-499：用户配置式 Hook
    /// - 500+：WASM 插件 Hook
    fn priority(&self) -> u32;

    /// 执行 Hook 逻辑
    ///
    /// Hook 可以：
    /// 1. 读取 ctx.payload 获取当前事件数据
    /// 2. 修改 ctx.payload 改变后续行为（如修改消息、参数）
    /// 3. 调用 ctx.abort() 中止管线
    /// 4. 调用 ctx.skip() 跳过当前操作
    /// 5. 通过 ctx.set_metadata() 向后续 Hook 传递信息
    async fn execute(&self, ctx: &mut HookContext) -> HookResult;

    /// 是否启用（可动态控制）
    fn enabled(&self) -> bool {
        true
    }

    /// Hook 描述（用于调试和 UI 展示）
    fn description(&self) -> &str {
        ""
    }
}
```

### 3.2 优先级分段规范

```text
优先级分段（数值越小越先执行）

  0 ─── 系统级（System）──── 99
        ObservabilityHook (50)     — Span 开始/结束
        SystemInitHook (10)        — 系统初始化

100 ─── 安全级（Security）── 199
        SecurityCheckHook (100)    — PathGuard/DomainGuard/ShellGuard
        PromptGuardHook (110)      — 提示注入扫描
        RateLimiterHook (120)      — 速率限制

200 ─── 隐私级（Privacy）── 299
        PrivacyFilterHook (200)    — PII 检测与脱敏
        AuditLogHook (250)         — 审计日志记录

300 ─── 业务级（Business）── 399
        BudgetCheckHook (300)      — 预算检查
        SkillInjectionHook (350)   — Skill 自动召回注入

400 ─── 用户配置级 ────────── 499
        ShellHook (400+)           — hooks.toml 定义的 shell 命令

500 ─── 插件级 ────────────── 999
        WasmPluginHook (500+)      — WASM 插件注册的 Hook
```

---

## 4. HookRegistry 设计

`HookRegistry` 是 Hook 的全局注册表，管理 Hook 的生命周期（注册、注销、启停）以及按事件查询已注册 Hook。

```rust
// crates/agent-core/src/hooks/registry.rs

use dashmap::DashMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// Hook 注册表：管理所有已注册的 Hook
pub struct HookRegistry {
    /// 所有已注册的 Hook（name → HookEntry）
    hooks: DashMap<String, HookEntry>,
    /// 事件 → Hook 列表的索引（缓存，避免每次查询都遍历全表）
    /// 在 Hook 注册/注销时重建
    event_index: RwLock<HashMap<HookEvent, Vec<IndexEntry>>>,
}

/// Hook 注册条目
struct HookEntry {
    hook: Arc<dyn Hook>,
    /// 运行时启停控制（覆盖 Hook::enabled()）
    runtime_enabled: AtomicBool,
    /// 注册来源
    source: HookSource,
    /// 注册时间
    registered_at: Instant,
}

/// 索引条目（按优先级排序后缓存）
struct IndexEntry {
    name: String,
    priority: u32,
}

/// Hook 来源
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookSource {
    /// 内置 Hook（随二进制分发）
    Builtin,
    /// 用户配置文件（hooks.toml）
    Config,
    /// WASM 插件注册
    Plugin { plugin_id: String },
    /// 运行时动态注册（如 Skill 注入）
    Runtime,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self {
            hooks: DashMap::new(),
            event_index: RwLock::new(HashMap::new()),
        }
    }

    /// 注册 Hook
    ///
    /// 如果已有同名 Hook，返回错误（不允许重复注册）
    pub fn register(
        &self,
        hook: Arc<dyn Hook>,
        source: HookSource,
    ) -> Result<(), HookRegistryError> {
        let name = hook.name().to_string();

        if self.hooks.contains_key(&name) {
            return Err(HookRegistryError::DuplicateName(name));
        }

        // 验证优先级范围（插件 Hook 不可使用系统保留范围）
        if matches!(source, HookSource::Plugin { .. }) && hook.priority() < 500 {
            return Err(HookRegistryError::PriorityReserved {
                hook_name: name,
                priority: hook.priority(),
                min_allowed: 500,
            });
        }

        if matches!(source, HookSource::Config) && hook.priority() < 400 {
            return Err(HookRegistryError::PriorityReserved {
                hook_name: name,
                priority: hook.priority(),
                min_allowed: 400,
            });
        }

        self.hooks.insert(name, HookEntry {
            hook,
            runtime_enabled: AtomicBool::new(true),
            source,
            registered_at: Instant::now(),
        });

        self.rebuild_index();
        Ok(())
    }

    /// 注销 Hook
    pub fn unregister(&self, name: &str) -> Result<(), HookRegistryError> {
        // 内置 Hook 不可注销
        if let Some(entry) = self.hooks.get(name) {
            if matches!(entry.source, HookSource::Builtin) {
                return Err(HookRegistryError::CannotUnregisterBuiltin(name.to_string()));
            }
        }

        self.hooks.remove(name)
            .ok_or_else(|| HookRegistryError::NotFound(name.to_string()))?;

        self.rebuild_index();
        Ok(())
    }

    /// 启用/禁用 Hook（运行时控制，不影响注册状态）
    pub fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), HookRegistryError> {
        let entry = self.hooks.get(name)
            .ok_or_else(|| HookRegistryError::NotFound(name.to_string()))?;
        entry.runtime_enabled.store(enabled, Ordering::Relaxed);
        Ok(())
    }

    /// 获取指定事件的所有已启用 Hook（按优先级排序）
    pub fn hooks_for_event(&self, event: &HookEvent) -> Vec<Arc<dyn Hook>> {
        let index = self.event_index.read();
        let entries = match index.get(event) {
            Some(entries) => entries,
            None => return vec![],
        };

        entries.iter()
            .filter_map(|entry| {
                let hook_entry = self.hooks.get(&entry.name)?;
                if !hook_entry.runtime_enabled.load(Ordering::Relaxed) {
                    return None;
                }
                if !hook_entry.hook.enabled() {
                    return None;
                }
                Some(hook_entry.hook.clone())
            })
            .collect()
    }

    /// 列出所有已注册的 Hook（用于 UI 展示和调试）
    pub fn list_all(&self) -> Vec<HookInfo> {
        self.hooks.iter().map(|entry| {
            let hook = &entry.value();
            HookInfo {
                name: hook.hook.name().to_string(),
                description: hook.hook.description().to_string(),
                events: hook.hook.events(),
                priority: hook.hook.priority(),
                enabled: hook.runtime_enabled.load(Ordering::Relaxed) && hook.hook.enabled(),
                source: hook.source.clone(),
            }
        }).collect()
    }

    /// 重建事件索引（在 Hook 注册/注销后调用）
    fn rebuild_index(&self) {
        let mut index: HashMap<HookEvent, Vec<IndexEntry>> = HashMap::new();

        for entry in self.hooks.iter() {
            let hook = &entry.value().hook;
            for event in hook.events() {
                index.entry(event).or_default().push(IndexEntry {
                    name: hook.name().to_string(),
                    priority: hook.priority(),
                });
            }
        }

        // 每个事件的 Hook 列表按优先级排序
        for list in index.values_mut() {
            list.sort_by_key(|e| e.priority);
        }

        *self.event_index.write() = index;
    }
}

/// Hook 信息（前端展示用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookInfo {
    pub name: String,
    pub description: String,
    pub events: Vec<HookEvent>,
    pub priority: u32,
    pub enabled: bool,
    pub source: HookSource,
}

/// 注册表错误
#[derive(Debug, thiserror::Error)]
pub enum HookRegistryError {
    #[error("Hook '{0}' 已存在")]
    DuplicateName(String),

    #[error("Hook '{0}' 不存在")]
    NotFound(String),

    #[error("内置 Hook '{0}' 不可注销")]
    CannotUnregisterBuiltin(String),

    #[error("Hook '{hook_name}' 优先级 {priority} 低于允许的最小值 {min_allowed}")]
    PriorityReserved {
        hook_name: String,
        priority: u32,
        min_allowed: u32,
    },
}
```

---

## 5. Hook 执行管线

`HookPipeline` 负责在特定事件触发时，按优先级顺序执行所有已注册的 Hook。

```rust
// crates/agent-core/src/hooks/pipeline.rs

use tokio::time::{timeout, Duration};
use tracing;

/// Hook 管线配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// 单个 Hook 的默认超时（毫秒）
    pub default_timeout_ms: u64,
    /// 默认失败策略
    pub default_failure_policy: FailurePolicy,
    /// 是否记录 Hook 执行追踪
    pub trace_execution: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: 5_000,
            default_failure_policy: FailurePolicy::FailOpen,
            trace_execution: true,
        }
    }
}

/// Hook 失败处理策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FailurePolicy {
    /// 失败时跳过该 Hook，继续执行后续 Hook（默认）
    FailOpen,
    /// 失败时中止整个管线
    FailClosed,
}

/// Hook 执行记录（用于调试和可观测性）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookExecutionRecord {
    /// Hook 名称
    pub hook_name: String,
    /// 事件类型
    pub event: HookEvent,
    /// 执行结果
    pub result: String,
    /// 执行耗时（微秒）
    pub duration_us: u64,
    /// 是否修改了 payload
    pub modified_payload: bool,
    /// 错误信息（如果有）
    pub error: Option<String>,
    /// 执行时间戳
    pub timestamp: i64,
}

/// Hook 管线执行报告
#[derive(Debug, Clone, Serialize)]
pub struct PipelineReport {
    pub event: HookEvent,
    pub hooks_executed: u32,
    pub hooks_skipped: u32,
    pub hooks_failed: u32,
    pub total_duration_us: u64,
    pub aborted: bool,
    pub abort_reason: Option<AbortReason>,
    pub records: Vec<HookExecutionRecord>,
}

/// Hook 执行管线
pub struct HookPipeline {
    registry: Arc<HookRegistry>,
    config: PipelineConfig,
    /// Hook 执行日志（内存环形缓冲区，保留最近 1000 条）
    execution_log: parking_lot::Mutex<VecDeque<HookExecutionRecord>>,
}

impl HookPipeline {
    pub fn new(registry: Arc<HookRegistry>, config: PipelineConfig) -> Self {
        Self {
            registry,
            config,
            execution_log: parking_lot::Mutex::new(VecDeque::with_capacity(1000)),
        }
    }

    /// 触发事件，按优先级顺序执行所有匹配的 Hook
    ///
    /// 返回管线执行报告，调用方根据 `ctx.is_aborted()` 和 `ctx.should_skip()` 决定后续行为。
    pub async fn execute(
        &self,
        event: HookEvent,
        ctx: &mut HookContext,
    ) -> PipelineReport {
        let hooks = self.registry.hooks_for_event(&event);
        let pipeline_start = Instant::now();

        let mut report = PipelineReport {
            event: event.clone(),
            hooks_executed: 0,
            hooks_skipped: 0,
            hooks_failed: 0,
            total_duration_us: 0,
            aborted: false,
            abort_reason: None,
            records: Vec::with_capacity(hooks.len()),
        };

        if hooks.is_empty() {
            return report;
        }

        for hook in &hooks {
            // 检查是否已被前一个 Hook 中止
            if ctx.is_aborted() {
                report.aborted = true;
                report.abort_reason = ctx.abort_reason().cloned();
                report.hooks_skipped += 1;
                continue;
            }

            let hook_start = Instant::now();
            let hook_name = hook.name().to_string();

            // 带超时的 Hook 执行
            let timeout_duration = Duration::from_millis(self.config.default_timeout_ms);
            let result = timeout(timeout_duration, hook.execute(ctx)).await;

            let duration_us = hook_start.elapsed().as_micros() as u64;

            let record = match result {
                Ok(HookResult::Continue) => {
                    report.hooks_executed += 1;
                    HookExecutionRecord {
                        hook_name: hook_name.clone(),
                        event: event.clone(),
                        result: "continue".into(),
                        duration_us,
                        modified_payload: false,  // 简化：无法精确追踪
                        error: None,
                        timestamp: now_ms(),
                    }
                }
                Ok(HookResult::Skip) => {
                    report.hooks_skipped += 1;
                    HookExecutionRecord {
                        hook_name: hook_name.clone(),
                        event: event.clone(),
                        result: "skip".into(),
                        duration_us,
                        modified_payload: false,
                        error: None,
                        timestamp: now_ms(),
                    }
                }
                Ok(HookResult::Abort { reason }) => {
                    ctx.abort(&hook_name, &reason);
                    report.aborted = true;
                    report.abort_reason = Some(AbortReason {
                        hook_name: hook_name.clone(),
                        message: reason.clone(),
                    });
                    report.hooks_executed += 1;
                    HookExecutionRecord {
                        hook_name: hook_name.clone(),
                        event: event.clone(),
                        result: "abort".into(),
                        duration_us,
                        modified_payload: false,
                        error: Some(reason),
                        timestamp: now_ms(),
                    }
                }
                Ok(HookResult::Error { message }) => {
                    report.hooks_failed += 1;
                    tracing::warn!(
                        hook = %hook_name, event = ?event,
                        "Hook 执行失败: {}", message
                    );

                    // 根据失败策略处理
                    match self.config.default_failure_policy {
                        FailurePolicy::FailOpen => {
                            // 跳过此 Hook，继续执行
                        }
                        FailurePolicy::FailClosed => {
                            ctx.abort(&hook_name, &format!("Hook 失败（fail-closed）: {}", message));
                            report.aborted = true;
                        }
                    }

                    HookExecutionRecord {
                        hook_name: hook_name.clone(),
                        event: event.clone(),
                        result: "error".into(),
                        duration_us,
                        modified_payload: false,
                        error: Some(message),
                        timestamp: now_ms(),
                    }
                }
                Err(_) => {
                    // 超时
                    report.hooks_failed += 1;
                    tracing::warn!(
                        hook = %hook_name, event = ?event,
                        timeout_ms = self.config.default_timeout_ms,
                        "Hook 执行超时"
                    );

                    HookExecutionRecord {
                        hook_name: hook_name.clone(),
                        event: event.clone(),
                        result: "timeout".into(),
                        duration_us: self.config.default_timeout_ms * 1000,
                        modified_payload: false,
                        error: Some(format!("超时（{}ms）", self.config.default_timeout_ms)),
                        timestamp: now_ms(),
                    }
                }
            };

            // 记录到执行日志
            if self.config.trace_execution {
                let mut log = self.execution_log.lock();
                if log.len() >= 1000 {
                    log.pop_front();
                }
                log.push_back(record.clone());
            }

            report.records.push(record);
        }

        report.total_duration_us = pipeline_start.elapsed().as_micros() as u64;
        report
    }

    /// 获取最近的 Hook 执行日志
    pub fn recent_execution_log(&self, limit: usize) -> Vec<HookExecutionRecord> {
        let log = self.execution_log.lock();
        log.iter().rev().take(limit).cloned().collect()
    }

    /// 清空执行日志
    pub fn clear_execution_log(&self) {
        self.execution_log.lock().clear();
    }
}
```

---

## 6. 内置 Hooks

现有的硬编码拦截器重构为 Hook trait 实现。每个内置 Hook 封装已有的逻辑，通过 HookRegistry 注册，使用保留优先级范围。

### 6.1 ObservabilityHook（优先级 50）

将现有的 SpanCollector / MetricsCollector 埋点逻辑封装为 Hook，监听所有阶段性事件。

```rust
// crates/agent-core/src/hooks/builtin/observability.rs

pub struct ObservabilityHook {
    span_collector: Arc<SpanCollector>,
    metrics_collector: Arc<MetricsCollector>,
}

impl ObservabilityHook {
    pub fn new(
        span_collector: Arc<SpanCollector>,
        metrics_collector: Arc<MetricsCollector>,
    ) -> Self {
        Self { span_collector, metrics_collector }
    }
}

#[async_trait]
impl Hook for ObservabilityHook {
    fn name(&self) -> &str { "builtin:observability" }

    fn events(&self) -> Vec<HookEvent> {
        vec![
            HookEvent::OnTurnStart,
            HookEvent::OnTurnEnd,
            HookEvent::BeforeLlmCall,
            HookEvent::AfterLlmCall,
            HookEvent::BeforeToolExecute,
            HookEvent::AfterToolExecute,
            HookEvent::OnToolError,
            HookEvent::OnTurnError,
        ]
    }

    fn priority(&self) -> u32 { 50 }

    fn description(&self) -> &str {
        "可观测性 Hook：创建 Span 树和性能指标采集"
    }

    async fn execute(&self, ctx: &mut HookContext) -> HookResult {
        match &ctx.event {
            HookEvent::OnTurnStart => {
                let span_id = self.span_collector.start_span(
                    "round_loop",
                    SpanKind::RoundLoop,
                    &ctx.conversation_id,
                    ctx.turn_index.unwrap_or(0),
                    None,
                );
                ctx.set_metadata("root_span_id", serde_json::json!(span_id));
            }
            HookEvent::OnTurnEnd => {
                if let Some(Value::String(span_id)) = ctx.get_metadata("root_span_id") {
                    self.span_collector.end_span(span_id, SpanStatus::Ok);
                }
                if let HookPayload::TurnData { duration_ms: Some(ms), .. } = &ctx.payload {
                    self.metrics_collector.record_round_latency(*ms as f64);
                }
            }
            HookEvent::BeforeLlmCall => {
                let root_span = ctx.get_metadata("root_span_id")
                    .and_then(|v| v.as_str()).map(String::from);
                let span_id = self.span_collector.start_span(
                    "llm_call", SpanKind::LlmCall,
                    &ctx.conversation_id,
                    ctx.turn_index.unwrap_or(0),
                    root_span.as_deref(),
                );
                ctx.set_metadata("llm_span_id", serde_json::json!(span_id));
                ctx.set_metadata("llm_call_start_ms", serde_json::json!(now_ms()));
            }
            HookEvent::AfterLlmCall => {
                if let Some(Value::String(span_id)) = ctx.get_metadata("llm_span_id") {
                    if let HookPayload::LlmResponse { usage, model, .. } = &ctx.payload {
                        self.span_collector.set_attribute(
                            &span_id, "model", serde_json::json!(model));
                        self.span_collector.set_attribute(
                            &span_id, "prompt_tokens", serde_json::json!(usage.prompt_tokens));
                        self.span_collector.set_attribute(
                            &span_id, "completion_tokens", serde_json::json!(usage.completion_tokens));
                    }
                    self.span_collector.end_span(&span_id, SpanStatus::Ok);

                    // 记录 LLM 指标
                    let start_ms = ctx.get_metadata("llm_call_start_ms")
                        .and_then(|v| v.as_i64()).unwrap_or(0);
                    let elapsed = (now_ms() - start_ms) as f64;
                    if let HookPayload::LlmResponse { usage, .. } = &ctx.payload {
                        let tps = if elapsed > 0.0 {
                            usage.completion_tokens as f64 / (elapsed / 1000.0)
                        } else { 0.0 };
                        self.metrics_collector.record_llm_call(0.0, elapsed, tps);
                    }
                }
            }
            HookEvent::BeforeToolExecute => {
                let root_span = ctx.get_metadata("root_span_id")
                    .and_then(|v| v.as_str()).map(String::from);
                if let HookPayload::ToolRequest { tool_name, .. } = &ctx.payload {
                    let span_id = self.span_collector.start_span(
                        &format!("tool_execution:{}", tool_name),
                        SpanKind::ToolExecution,
                        &ctx.conversation_id,
                        ctx.turn_index.unwrap_or(0),
                        root_span.as_deref(),
                    );
                    ctx.set_metadata("tool_span_id", serde_json::json!(span_id));
                    ctx.set_metadata("tool_start_ms", serde_json::json!(now_ms()));
                }
            }
            HookEvent::AfterToolExecute => {
                if let Some(Value::String(span_id)) = ctx.get_metadata("tool_span_id") {
                    self.span_collector.end_span(&span_id, SpanStatus::Ok);
                    let start = ctx.get_metadata("tool_start_ms")
                        .and_then(|v| v.as_i64()).unwrap_or(0);
                    self.metrics_collector.record_tool_exec((now_ms() - start) as f64);
                }
            }
            HookEvent::OnToolError | HookEvent::OnTurnError => {
                if let Some(Value::String(span_id)) = ctx.get_metadata("tool_span_id") {
                    let error_msg = match &ctx.payload {
                        HookPayload::ToolResponse { result, .. } if result.is_error =>
                            "tool execution error".to_string(),
                        HookPayload::TurnData { error: Some(e), .. } => e.clone(),
                        _ => "unknown error".to_string(),
                    };
                    self.span_collector.end_span(&span_id, SpanStatus::Error(error_msg));
                }
            }
            _ => {}
        }
        HookResult::Continue
    }
}
```

### 6.2 SecurityCheckHook（优先级 100）

封装 PathGuard、DomainGuard、ShellGuard 的安全策略评估。

```rust
// crates/agent-core/src/hooks/builtin/security.rs

pub struct SecurityCheckHook {
    policy: Arc<SecurityPolicy>,
}

#[async_trait]
impl Hook for SecurityCheckHook {
    fn name(&self) -> &str { "builtin:security_check" }

    fn events(&self) -> Vec<HookEvent> {
        vec![HookEvent::BeforeToolExecute]
    }

    fn priority(&self) -> u32 { 100 }

    fn description(&self) -> &str {
        "安全检查 Hook：PathGuard / DomainGuard / ShellGuard 策略评估"
    }

    async fn execute(&self, ctx: &mut HookContext) -> HookResult {
        if let HookPayload::ToolRequest { tool_name, arguments, risk_level, .. } = &mut ctx.payload {
            // 路径检查
            if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                let op = match tool_name.as_str() {
                    "file_read" | "file_list" | "file_search" => FileOperation::Read,
                    "file_delete" => FileOperation::Delete,
                    _ => FileOperation::Write,
                };
                match self.policy.path_guard().check(Path::new(path), op) {
                    Ok(result) => {
                        *risk_level = (*risk_level).max(result.risk_level);
                    }
                    Err(e) => {
                        return HookResult::Abort {
                            reason: format!("安全策略拒绝: {}", e),
                        };
                    }
                }
            }

            // 域名检查
            if let Some(url) = arguments.get("url").and_then(|v| v.as_str()) {
                match self.policy.domain_guard().check(url) {
                    Ok(result) => {
                        *risk_level = (*risk_level).max(result.risk_level);
                    }
                    Err(e) => {
                        return HookResult::Abort {
                            reason: format!("安全策略拒绝: {}", e),
                        };
                    }
                }
            }

            // Shell 命令检查
            if let Some(cmd) = arguments.get("command").and_then(|v| v.as_str()) {
                match self.policy.shell_guard().check(cmd) {
                    Ok(result) => {
                        *risk_level = (*risk_level).max(result.risk_level);
                    }
                    Err(e) => {
                        return HookResult::Abort {
                            reason: format!("安全策略拒绝: {}", e),
                        };
                    }
                }
            }
        }
        HookResult::Continue
    }
}
```

### 6.3 PrivacyFilterHook（优先级 200）

封装 SensitiveFilter，在 LLM 调用前对消息内容执行 PII 检测和脱敏。

```rust
// crates/agent-core/src/hooks/builtin/privacy.rs

pub struct PrivacyFilterHook {
    filter: Arc<SensitiveFilter>,
    emitter: Arc<dyn EventEmitter>,
}

#[async_trait]
impl Hook for PrivacyFilterHook {
    fn name(&self) -> &str { "builtin:privacy_filter" }

    fn events(&self) -> Vec<HookEvent> {
        vec![HookEvent::BeforeLlmCall]
    }

    fn priority(&self) -> u32 { 200 }

    fn description(&self) -> &str {
        "隐私过滤 Hook：PII 检测与脱敏（手机号、身份证、API Key 等）"
    }

    async fn execute(&self, ctx: &mut HookContext) -> HookResult {
        if let HookPayload::LlmRequest { messages, .. } = &mut ctx.payload {
            let mut total_redacted = 0;
            let mut all_warnings = Vec::new();

            for msg in messages.iter_mut() {
                // 不过滤 system prompt（应用自身生成）
                if msg.role == "system" { continue; }

                let result = self.filter.apply(&msg.content);
                match result {
                    FilterResult::Blocked { matches } => {
                        let rules: Vec<String> = matches.iter()
                            .filter(|m| matches!(m.action, FilterAction::Block))
                            .map(|m| m.rule_name.clone())
                            .collect();
                        return HookResult::Abort {
                            reason: format!(
                                "隐私过滤阻断: 消息包含敏感内容 ({})",
                                rules.join(", ")
                            ),
                        };
                    }
                    FilterResult::Allowed { content, warnings, redacted_count } => {
                        msg.content = content;
                        total_redacted += redacted_count;
                        all_warnings.extend(warnings);
                    }
                }
            }

            // 通知前端
            if total_redacted > 0 || !all_warnings.is_empty() {
                self.emitter.emit("privacy_filter_applied", serde_json::json!({
                    "conversation_id": ctx.conversation_id,
                    "redacted_count": total_redacted,
                    "warning_count": all_warnings.len(),
                })).await.ok();
            }
        }
        HookResult::Continue
    }
}
```

### 6.4 BudgetCheckHook（优先级 300）

封装 BudgetManager 的预算检查逻辑。

```rust
// crates/agent-core/src/hooks/builtin/budget.rs

pub struct BudgetCheckHook {
    budget_manager: Arc<BudgetManager>,
}

#[async_trait]
impl Hook for BudgetCheckHook {
    fn name(&self) -> &str { "builtin:budget_check" }

    fn events(&self) -> Vec<HookEvent> {
        vec![HookEvent::BeforeLlmCall]
    }

    fn priority(&self) -> u32 { 300 }

    fn description(&self) -> &str {
        "预算检查 Hook：LLM 调用前验证 Token / 费用预算"
    }

    async fn execute(&self, ctx: &mut HookContext) -> HookResult {
        if let HookPayload::LlmRequest { estimated_input_tokens, .. } = &ctx.payload {
            let estimated_cost = *estimated_input_tokens as f64 * 0.000003; // 简化估算
            match self.budget_manager
                .check_before_call(&ctx.workspace_id, estimated_cost).await
            {
                Ok(()) => HookResult::Continue,
                Err(e) => HookResult::Abort {
                    reason: format!("预算超限: {}", e),
                },
            }
        } else {
            HookResult::Continue
        }
    }
}
```

### 6.5 AuditLogHook（优先级 250）

在工具执行后写入审计日志。

```rust
// crates/agent-core/src/hooks/builtin/audit.rs

pub struct AuditLogHook {
    audit_store: Arc<SecurityAuditStore>,
}

#[async_trait]
impl Hook for AuditLogHook {
    fn name(&self) -> &str { "builtin:audit_log" }

    fn events(&self) -> Vec<HookEvent> {
        vec![
            HookEvent::AfterToolExecute,
            HookEvent::OnToolError,
        ]
    }

    fn priority(&self) -> u32 { 250 }

    fn description(&self) -> &str {
        "审计日志 Hook：记录工具调用及结果"
    }

    async fn execute(&self, ctx: &mut HookContext) -> HookResult {
        if let HookPayload::ToolResponse {
            tool_name, arguments, duration_ms, is_error, ..
        } = &ctx.payload {
            // 仅审计 High/Critical 风险或失败的工具调用
            let should_audit = *is_error
                || ctx.get_metadata("risk_level")
                    .and_then(|v| v.as_str())
                    .map(|r| r == "High" || r == "Critical")
                    .unwrap_or(false);

            if should_audit {
                let event_type = if *is_error {
                    SecurityEventType::HighRiskToolCall
                } else {
                    SecurityEventType::HighRiskToolCall
                };

                self.audit_store.log(SecurityAuditLog {
                    id: Uuid::new_v4(),
                    timestamp: Utc::now(),
                    event_type,
                    tool_name: Some(tool_name.clone()),
                    severity: if *is_error { "Error".into() } else { "Info".into() },
                    details: serde_json::json!({
                        "duration_ms": duration_ms,
                        "is_error": is_error,
                        "args_keys": arguments.as_object()
                            .map(|o| o.keys().cloned().collect::<Vec<_>>())
                            .unwrap_or_default(),
                    }),
                    action_taken: "logged".into(),
                }).await.ok();
            }
        }
        HookResult::Continue
    }
}
```

### 6.6 PromptGuardHook（优先级 110）

封装提示注入扫描，在工具输出写入上下文前执行。

```rust
// crates/agent-core/src/hooks/builtin/prompt_guard.rs

pub struct PromptGuardHook {
    guard: Arc<PromptGuard>,
    emitter: Arc<dyn EventEmitter>,
}

#[async_trait]
impl Hook for PromptGuardHook {
    fn name(&self) -> &str { "builtin:prompt_guard" }

    fn events(&self) -> Vec<HookEvent> {
        vec![HookEvent::AfterToolExecute]
    }

    fn priority(&self) -> u32 { 110 }

    fn description(&self) -> &str {
        "提示注入防御 Hook：扫描工具输出中的恶意指令"
    }

    async fn execute(&self, ctx: &mut HookContext) -> HookResult {
        if let HookPayload::ToolResponse { tool_name, result, .. } = &ctx.payload {
            let content = serde_json::to_string(&result.output).unwrap_or_default();
            let warnings = self.guard.scan_tool_output(&content);

            if !warnings.is_empty() {
                let max_severity = warnings.iter()
                    .map(|w| &w.severity)
                    .max()
                    .cloned()
                    .unwrap_or(InjectionSeverity::Low);

                // 记录告警到元数据（后续 AuditLogHook 可读取）
                ctx.set_metadata("injection_warnings", serde_json::json!({
                    "count": warnings.len(),
                    "max_severity": format!("{:?}", max_severity),
                }));

                // 通知前端
                self.emitter.emit("injection_warning", serde_json::json!({
                    "tool_name": tool_name,
                    "warning_count": warnings.len(),
                    "max_severity": format!("{:?}", max_severity),
                })).await.ok();

                // 高风险注入不中止（避免误杀），但提升风险等级
                if max_severity == InjectionSeverity::High {
                    ctx.set_metadata("risk_level", serde_json::json!("Critical"));
                }
            }
        }
        HookResult::Continue
    }
}
```

### 6.7 内置 Hook 注册

应用启动时，所有内置 Hook 在 `HookRegistry` 中注册：

```rust
// crates/agent-runtime/src/hooks/integration.rs

pub fn register_builtin_hooks(
    registry: &HookRegistry,
    span_collector: Arc<SpanCollector>,
    metrics_collector: Arc<MetricsCollector>,
    security_policy: Arc<SecurityPolicy>,
    sensitive_filter: Arc<SensitiveFilter>,
    budget_manager: Arc<BudgetManager>,
    audit_store: Arc<SecurityAuditStore>,
    prompt_guard: Arc<PromptGuard>,
    emitter: Arc<dyn EventEmitter>,
) -> anyhow::Result<()> {
    registry.register(
        Arc::new(ObservabilityHook::new(span_collector, metrics_collector)),
        HookSource::Builtin,
    )?;

    registry.register(
        Arc::new(SecurityCheckHook { policy: security_policy }),
        HookSource::Builtin,
    )?;

    registry.register(
        Arc::new(PromptGuardHook { guard: prompt_guard, emitter: emitter.clone() }),
        HookSource::Builtin,
    )?;

    registry.register(
        Arc::new(PrivacyFilterHook { filter: sensitive_filter, emitter: emitter.clone() }),
        HookSource::Builtin,
    )?;

    registry.register(
        Arc::new(AuditLogHook { audit_store }),
        HookSource::Builtin,
    )?;

    registry.register(
        Arc::new(BudgetCheckHook { budget_manager }),
        HookSource::Builtin,
    )?;

    Ok(())
}
```

---

## 7. 用户自定义 Hooks

用户可通过两种机制定义自己的 Hook：配置式（shell 命令）和编程式（WASM 插件）。

### 7.1 配置式 Hooks（ShellHook）

受 Claude Code Hooks 启发，用户通过 TOML 配置文件定义在特定事件触发时执行的 shell 命令。

```rust
// crates/agent-core/src/hooks/shell_hook.rs

/// 配置式 Shell Hook
pub struct ShellHook {
    config: ShellHookConfig,
}

/// Shell Hook 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellHookConfig {
    /// Hook 名称
    pub name: String,
    /// 监听的事件
    pub event: HookEvent,
    /// 可选的工具名过滤（仅在 Tool 级事件生效）
    pub tool_filter: Option<String>,
    /// 要执行的 shell 命令（支持环境变量模板）
    pub command: String,
    /// 超时（毫秒）
    pub timeout_ms: u64,
    /// 失败处理策略
    pub on_failure: ShellHookFailureAction,
    /// 优先级（400-499 范围）
    pub priority: u32,
    /// 是否启用
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellHookFailureAction {
    /// 命令失败时中止管线
    Abort,
    /// 命令失败时继续执行
    Continue,
    /// 命令失败时跳过当前操作
    Skip,
}

#[async_trait]
impl Hook for ShellHook {
    fn name(&self) -> &str { &self.config.name }

    fn events(&self) -> Vec<HookEvent> {
        vec![self.config.event.clone()]
    }

    fn priority(&self) -> u32 {
        self.config.priority.clamp(400, 499)
    }

    fn enabled(&self) -> bool {
        self.config.enabled
    }

    fn description(&self) -> &str {
        "用户配置的 Shell 命令 Hook"
    }

    async fn execute(&self, ctx: &mut HookContext) -> HookResult {
        // 工具名过滤
        if let Some(filter) = &self.config.tool_filter {
            if let HookPayload::ToolRequest { tool_name, .. } = &ctx.payload {
                if tool_name != filter {
                    return HookResult::Skip;
                }
            }
        }

        // 构建环境变量
        let env = self.build_env(ctx);

        // 执行 shell 命令
        let result = tokio::time::timeout(
            Duration::from_millis(self.config.timeout_ms),
            exec_shell_with_env(&self.config.command, &env),
        ).await;

        match result {
            Ok(Ok(output)) => {
                if output.exit_code == 0 {
                    // 如果命令有 stdout 输出，写入元数据供后续 Hook 使用
                    if !output.stdout.is_empty() {
                        ctx.set_metadata(
                            &format!("shell_hook:{}", self.config.name),
                            serde_json::json!(output.stdout),
                        );
                    }
                    HookResult::Continue
                } else {
                    match self.config.on_failure {
                        ShellHookFailureAction::Abort => HookResult::Abort {
                            reason: format!(
                                "Shell Hook '{}' 失败（exit {}）: {}",
                                self.config.name, output.exit_code, output.stderr
                            ),
                        },
                        ShellHookFailureAction::Continue => {
                            tracing::warn!(
                                hook = %self.config.name,
                                "Shell Hook 失败（exit {}），继续执行",
                                output.exit_code
                            );
                            HookResult::Continue
                        }
                        ShellHookFailureAction::Skip => {
                            ctx.skip();
                            HookResult::Continue
                        }
                    }
                }
            }
            Ok(Err(e)) => HookResult::Error {
                message: format!("Shell Hook 执行错误: {}", e),
            },
            Err(_) => HookResult::Error {
                message: format!(
                    "Shell Hook '{}' 超时（{}ms）",
                    self.config.name, self.config.timeout_ms
                ),
            },
        }
    }
}

impl ShellHook {
    /// 构建 Shell 命令的环境变量
    fn build_env(&self, ctx: &HookContext) -> HashMap<String, String> {
        let mut env = HashMap::new();

        env.insert("ASTRO_EVENT".into(), format!("{:?}", ctx.event));
        env.insert("ASTRO_CONVERSATION_ID".into(), ctx.conversation_id.clone());
        env.insert("ASTRO_WORKSPACE_ID".into(), ctx.workspace_id.clone());

        match &ctx.payload {
            HookPayload::ToolRequest { tool_name, arguments, call_id, .. } => {
                env.insert("TOOL_NAME".into(), tool_name.clone());
                env.insert("TOOL_CALL_ID".into(), call_id.clone());
                env.insert("TOOL_ARGS".into(),
                    serde_json::to_string(arguments).unwrap_or_default());
            }
            HookPayload::ToolResponse { tool_name, duration_ms, is_error, .. } => {
                env.insert("TOOL_NAME".into(), tool_name.clone());
                env.insert("TOOL_DURATION_MS".into(), duration_ms.to_string());
                env.insert("TOOL_IS_ERROR".into(), is_error.to_string());
            }
            HookPayload::LlmRequest { model, estimated_input_tokens, .. } => {
                env.insert("LLM_MODEL".into(), model.clone());
                env.insert("LLM_INPUT_TOKENS".into(), estimated_input_tokens.to_string());
            }
            _ => {}
        }

        env
    }
}

/// 带环境变量的 Shell 执行
async fn exec_shell_with_env(
    command: &str,
    env: &HashMap<String, String>,
) -> anyhow::Result<ShellOutput> {
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(command);
    for (key, val) in env {
        cmd.env(key, val);
    }
    cmd.stdout(Stdio::piped())
       .stderr(Stdio::piped());

    let output = cmd.output().await?;
    Ok(ShellOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}
```

### 7.2 WASM 插件 Hooks

WASM 插件通过宿主函数 `astro_register_hook` 注册 Hook，在沙箱内执行。

```rust
// crates/agent-runtime/src/hooks/wasm_hook.rs

/// WASM 插件注册的 Hook
pub struct WasmPluginHook {
    plugin_id: String,
    hook_name: String,
    event: HookEvent,
    priority: u32,
    pool: Arc<PluginPool>,
    /// 插件内处理 Hook 事件的函数名
    handler_fn: String,
}

#[async_trait]
impl Hook for WasmPluginHook {
    fn name(&self) -> &str { &self.hook_name }

    fn events(&self) -> Vec<HookEvent> {
        vec![self.event.clone()]
    }

    fn priority(&self) -> u32 {
        self.priority.max(500) // 插件 Hook 强制最低优先级 500
    }

    fn description(&self) -> &str {
        "WASM 插件注册的 Hook"
    }

    async fn execute(&self, ctx: &mut HookContext) -> HookResult {
        // 序列化 HookContext 为 MessagePack
        let input = rmp_serde::to_vec(&HookContextDto::from(ctx))
            .unwrap_or_default();

        // 获取插件实例并调用 handler
        let mut inst = match self.pool.acquire(&self.plugin_id).await {
            Ok(inst) => inst,
            Err(e) => return HookResult::Error {
                message: format!("插件实例获取失败: {}", e),
            },
        };

        match call_plugin_fn(&mut inst, &self.handler_fn, &input).await {
            Ok(output) => {
                // 反序列化插件返回的 Hook 结果
                match rmp_serde::from_slice::<WasmHookResponse>(&output) {
                    Ok(resp) => {
                        // 应用插件对 payload 的修改（如果有）
                        if let Some(modified) = resp.modified_payload {
                            apply_wasm_modifications(ctx, modified);
                        }
                        match resp.action.as_str() {
                            "continue" => HookResult::Continue,
                            "abort" => HookResult::Abort {
                                reason: resp.message.unwrap_or_default(),
                            },
                            "skip" => HookResult::Skip,
                            _ => HookResult::Continue,
                        }
                    }
                    Err(e) => HookResult::Error {
                        message: format!("插件响应解码失败: {}", e),
                    },
                }
            }
            Err(e) => HookResult::Error {
                message: format!("插件 Hook 执行失败: {}", e),
            },
        }
    }
}

/// 插件 Hook 响应格式
#[derive(Debug, Deserialize)]
struct WasmHookResponse {
    action: String, // "continue" | "abort" | "skip"
    message: Option<String>,
    modified_payload: Option<Value>,
}
```

WASM 插件在 `plugin.toml` 中声明 Hook：

```toml
[plugin]
id = "my-linter"
name = "代码质量检查插件"
entry = "plugin.wasm"

[[hooks]]
event = "before_tool_execute"
tool = "file_write"
handler = "on_file_write"
priority = 550
```

---

## 8. Hook 配置文件格式

### 8.1 配置文件位置

Hook 配置文件支持两级：

| 位置 | 作用域 | 优先级 |
|------|--------|--------|
| `~/.astro/hooks.toml` | 全局（所有工作区生效） | 低 |
| `{workspace}/.astro/hooks.toml` | 工作区级 | 高（覆盖同名全局 Hook） |

### 8.2 完整配置格式

```toml
# ~/.astro/hooks.toml — 全局 Hook 配置

# ── 配置式 Shell Hooks ──

[[hooks]]
name = "log_tool_calls"
event = "after_tool_execute"
command = "echo \"$(date): $TOOL_NAME ($TOOL_DURATION_MS ms)\" >> ~/.astro/tool_calls.log"
timeout_ms = 2000
on_failure = "continue"
priority = 410
enabled = true

[[hooks]]
name = "security_scan_shell"
event = "before_tool_execute"
tool = "shell_exec"
command = "security-scan --command \"$TOOL_ARGS\""
timeout_ms = 3000
on_failure = "abort"
priority = 420
enabled = true

[[hooks]]
name = "notify_on_error"
event = "on_turn_error"
command = "osascript -e 'display notification \"Agent 执行出错\" with title \"Astro Agent\"'"
timeout_ms = 5000
on_failure = "continue"
priority = 450
enabled = true

[[hooks]]
name = "custom_privacy_check"
event = "before_llm_call"
command = "/usr/local/bin/privacy-check --stdin"
timeout_ms = 3000
on_failure = "abort"
priority = 430
enabled = false   # 默认禁用，用户手动启用

# ── 内置 Hook 的行为覆盖 ──

[builtin_overrides]
# 禁用内置的隐私过滤（如果用户有自己的隐私检查方案）
# "builtin:privacy_filter" = { enabled = false }

# 调整内置 Hook 的失败策略
# "builtin:budget_check" = { failure_policy = "fail_closed" }

# ── 管线全局配置 ──

[pipeline]
# 单个 Hook 的默认超时（毫秒）
default_timeout_ms = 5000
# 默认失败策略：fail_open 或 fail_closed
default_failure_policy = "fail_open"
# 是否记录 Hook 执行追踪
trace_execution = true
```

### 8.3 配置加载

```rust
// crates/agent-core/src/hooks/config.rs

use serde::Deserialize;

/// Hook 配置文件结构
#[derive(Debug, Clone, Deserialize)]
pub struct HookConfigFile {
    #[serde(default)]
    pub hooks: Vec<ShellHookConfig>,

    #[serde(default)]
    pub builtin_overrides: HashMap<String, BuiltinOverride>,

    #[serde(default)]
    pub pipeline: PipelineConfigOverride,
}

/// 内置 Hook 的行为覆盖
#[derive(Debug, Clone, Deserialize)]
pub struct BuiltinOverride {
    pub enabled: Option<bool>,
    pub failure_policy: Option<String>,
}

/// 管线配置覆盖
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PipelineConfigOverride {
    pub default_timeout_ms: Option<u64>,
    pub default_failure_policy: Option<String>,
    pub trace_execution: Option<bool>,
}

/// 加载并合并全局和工作区级 Hook 配置
pub fn load_hook_config(
    workspace_root: Option<&Path>,
) -> anyhow::Result<HookConfigFile> {
    let home = dirs::home_dir().unwrap_or_default();
    let global_path = home.join(".astro/hooks.toml");
    let workspace_path = workspace_root
        .map(|root| root.join(".astro/hooks.toml"));

    let mut config = if global_path.exists() {
        let content = std::fs::read_to_string(&global_path)?;
        toml::from_str::<HookConfigFile>(&content)?
    } else {
        HookConfigFile {
            hooks: vec![],
            builtin_overrides: HashMap::new(),
            pipeline: PipelineConfigOverride::default(),
        }
    };

    // 合并工作区级配置（工作区 Hook 追加，同名覆盖）
    if let Some(ws_path) = workspace_path {
        if ws_path.exists() {
            let ws_content = std::fs::read_to_string(&ws_path)?;
            let ws_config = toml::from_str::<HookConfigFile>(&ws_content)?;

            // 合并 hooks：工作区同名 Hook 覆盖全局
            let mut hook_map: HashMap<String, ShellHookConfig> = config.hooks
                .into_iter()
                .map(|h| (h.name.clone(), h))
                .collect();
            for h in ws_config.hooks {
                hook_map.insert(h.name.clone(), h);
            }
            config.hooks = hook_map.into_values().collect();

            // 合并内置覆盖
            config.builtin_overrides.extend(ws_config.builtin_overrides);

            // 管线配置取工作区值（如果有）
            if ws_config.pipeline.default_timeout_ms.is_some() {
                config.pipeline.default_timeout_ms = ws_config.pipeline.default_timeout_ms;
            }
            if ws_config.pipeline.default_failure_policy.is_some() {
                config.pipeline.default_failure_policy = ws_config.pipeline.default_failure_policy;
            }
        }
    }

    Ok(config)
}
```

---

## 9. 与 round_loop 的集成

### 9.1 Hook 注入点全景图

以下 ASCII 流程图标注了 `round_loop` 中每个 Hook 事件的触发位置，对应 [02-agent-runtime详细设计.md](02-agent-runtime详细设计.md) 第 4 节的主循环结构。

```text
AgentExecutor::new()
    │
    ├─ [H] OnAgentInit ─────────────── 初始化资源、验证配置
    │
    ▼
AgentExecutor::round_loop()
    │
    ├─ [H] OnAgentReady ─────────────── 首次进入循环前（仅一次）
    │
    │   ┌──────────────────────── round_loop 主循环 ─────────────────────┐
    │   │                                                                │
    │   ├─ [H] OnTurnStart ──────────── ① 轮次开始                      │
    │   │                                                                │
    │   ├─ ① pending 消息注入                                            │
    │   │       └─ [H] OnUserMessage ── 每条 pending 消息                │
    │   │                                                                │
    │   ├─ ② 上下文构建                                                  │
    │   │       ├─ [H] BeforeMemoryInject ── 记忆注入前                  │
    │   │       ├─ (embedding search)                                    │
    │   │       ├─ [H] AfterMemoryRetrieve ── 记忆检索后                 │
    │   │       └─ [H] OnSystemMessage ──── 系统提示注入时               │
    │   │                                                                │
    │   ├─ ③ LLM 调用                                                    │
    │   │       ├─ [H] BeforeLlmCall ─────── 可修改 messages/model       │
    │   │       │       ├─ 中止？→ 跳过 LLM 调用，返回错误              │
    │   │       │       └─ 继续 ↓                                        │
    │   │       ├─ provider.chat_stream()                                │
    │   │       │       └─ [H] OnLlmStreamChunk ── 每个流式 delta       │
    │   │       └─ [H] AfterLlmCall ──────── 响应审计、token 统计        │
    │   │                                                                │
    │   ├─ ④ 计划确认（如有 <agent_plan>）                                │
    │   │                                                                │
    │   ├─ ⑤ 工具执行（每个 ToolCall）                                    │
    │   │       ├─ [H] BeforeToolExecute ─── 安全检查、参数校验          │
    │   │       │       ├─ 中止？→ 注入拒绝 ToolResult，跳过执行        │
    │   │       │       └─ 继续 ↓                                        │
    │   │       ├─ HumanGuard::check()                                   │
    │   │       ├─ ToolRegistry::execute()                               │
    │   │       ├─ 成功？                                                │
    │   │       │   ├─ YES → [H] AfterToolExecute ── 输出扫描、审计     │
    │   │       │   └─ NO  → [H] OnToolError ──────── 错误分类、告警    │
    │   │       └─ [H] OnAssistantMessage ── Agent 消息生成时            │
    │   │                                                                │
    │   ├─ ⑥ 无更多工具调用？                                            │
    │   │       ├─ YES → [H] OnTurnEnd ──── 聚合指标、持久化             │
    │   │       │         └─ 退出循环                                    │
    │   │       └─ NO  → 回到 ①                                         │
    │   │                                                                │
    │   ├─ 捕获错误 → [H] OnTurnError ──── 错误上下文记录               │
    │   │                                                                │
    │   └────────────────────────────────────────────────────────────────┘
    │
    ├─ [H] OnAgentTerminate ──────────── 清理资源、写入最终统计
    │
    ▼
  round_loop 退出
```

### 9.2 集成代码

将 HookPipeline 集成到 `AgentExecutor` 中，替代原有的硬编码拦截逻辑。

```rust
// crates/agent-runtime/src/hooks/integration.rs

/// 在 AgentExecutor 中注入 HookPipeline
pub struct HookIntegration {
    pipeline: Arc<HookPipeline>,
}

impl HookIntegration {
    pub fn new(pipeline: Arc<HookPipeline>) -> Self {
        Self { pipeline }
    }

    /// 在 LLM 调用前执行 Hook 管线
    /// 返回可能被修改的消息和模型参数
    pub async fn before_llm_call(
        &self,
        conversation_id: &str,
        workspace_id: &str,
        turn_index: u32,
        messages: Vec<Message>,
        model: String,
        estimated_tokens: u64,
    ) -> Result<(Vec<Message>, String), HookAbortError> {
        let mut ctx = HookContext {
            event: HookEvent::BeforeLlmCall,
            conversation_id: conversation_id.to_string(),
            workspace_id: workspace_id.to_string(),
            agent_depth: 0,
            turn_index: Some(turn_index),
            payload: HookPayload::LlmRequest {
                messages: messages.clone(),
                model: model.clone(),
                temperature: None,
                estimated_input_tokens: estimated_tokens,
            },
            metadata: HashMap::new(),
            abort: None,
            skip: false,
        };

        let report = self.pipeline.execute(HookEvent::BeforeLlmCall, &mut ctx).await;

        if ctx.is_aborted() {
            return Err(HookAbortError {
                reason: ctx.abort_reason().unwrap().clone(),
            });
        }

        // 提取可能被 Hook 修改的数据
        if let HookPayload::LlmRequest { messages, model, .. } = ctx.payload {
            Ok((messages, model))
        } else {
            Ok((messages, model))
        }
    }

    /// 在工具执行前执行 Hook 管线
    pub async fn before_tool_execute(
        &self,
        conversation_id: &str,
        workspace_id: &str,
        turn_index: u32,
        tool_name: &str,
        arguments: Value,
        call_id: &str,
        risk_level: RiskLevel,
    ) -> Result<(Value, RiskLevel), HookAbortError> {
        let mut ctx = HookContext {
            event: HookEvent::BeforeToolExecute,
            conversation_id: conversation_id.to_string(),
            workspace_id: workspace_id.to_string(),
            agent_depth: 0,
            turn_index: Some(turn_index),
            payload: HookPayload::ToolRequest {
                tool_name: tool_name.to_string(),
                arguments: arguments.clone(),
                call_id: call_id.to_string(),
                risk_level,
            },
            metadata: HashMap::new(),
            abort: None,
            skip: false,
        };

        self.pipeline.execute(HookEvent::BeforeToolExecute, &mut ctx).await;

        if ctx.is_aborted() {
            return Err(HookAbortError {
                reason: ctx.abort_reason().unwrap().clone(),
            });
        }

        if let HookPayload::ToolRequest { arguments, risk_level, .. } = ctx.payload {
            Ok((arguments, risk_level))
        } else {
            Ok((arguments, risk_level))
        }
    }

    /// 触发通知性事件（不可中止，不修改数据）
    pub async fn notify(&self, event: HookEvent, ctx: &mut HookContext) {
        self.pipeline.execute(event, ctx).await;
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Hook 管线中止: {}", reason.message)]
pub struct HookAbortError {
    pub reason: AbortReason,
}
```

### 9.3 改造后的 round_loop（关键片段）

```rust
// crates/agent-runtime/src/executor.rs（改造后）

impl AgentExecutor {
    pub async fn round_loop(&self) -> Result<RoundOutcome> {
        let hooks = &self.hook_integration;  // HookIntegration

        // [H] OnAgentReady（仅首次）
        hooks.notify(HookEvent::OnAgentReady, &mut self.build_empty_ctx()).await;

        let mut round = 0;
        loop {
            round += 1;

            // [H] OnTurnStart
            hooks.notify(HookEvent::OnTurnStart, &mut self.build_turn_ctx(round)).await;

            // ① pending 注入
            for msg in self.pending_queue.drain().await {
                self.context.add_message(Message::user(msg.content, ContextSlot::Regular));
            }

            // ③ LLM 调用（通过 Hook 管线）
            let (messages, model) = hooks.before_llm_call(
                &self.context.conversation_id,
                &self.context.workspace_id,
                round,
                self.context.build_messages(),
                self.context.model.clone(),
                self.context.estimate_input_tokens(),
            ).await.map_err(|e| AgentError::HookAbort(e.reason.message))?;

            let response = self.context.llm_call_stream(&self.emitter).await?;

            // [H] AfterLlmCall（通知性）
            hooks.notify(HookEvent::AfterLlmCall, &mut self.build_llm_response_ctx(
                round, &response
            )).await;

            // ⑤ 工具执行
            let tool_calls = response.tool_calls();
            if tool_calls.is_empty() {
                hooks.notify(HookEvent::OnTurnEnd, &mut self.build_turn_end_ctx(round)).await;
                return Ok(RoundOutcome::Done);
            }

            for call in tool_calls {
                // [H] BeforeToolExecute（可中止）
                let (args, risk) = match hooks.before_tool_execute(
                    &self.context.conversation_id,
                    &self.context.workspace_id,
                    round,
                    &call.tool_name,
                    call.input.clone(),
                    &call.id,
                    call.risk_level,
                ).await {
                    Ok(result) => result,
                    Err(abort) => {
                        // Hook 中止 → 注入拒绝 ToolResult
                        let rejection = ToolResult {
                            call_id: call.id.clone(),
                            is_error: true,
                            output: vec![ToolContent::Text(
                                format!("操作被安全策略拒绝: {}", abort.reason.message)
                            )],
                        };
                        self.context.messages.push(Message::tool_result(rejection));
                        continue;
                    }
                };

                // HumanGuard 审批
                let outcome = self.human_guard.check(&call.tool, &args).await?;
                if outcome == ApprovalOutcome::Rejected {
                    continue;
                }

                // 实际执行
                let result = self.context.tools.execute(&call.tool_name, args).await;

                // [H] AfterToolExecute / OnToolError
                if result.is_error {
                    hooks.notify(HookEvent::OnToolError, &mut self.build_tool_error_ctx(
                        round, &call.tool_name, &result
                    )).await;
                } else {
                    hooks.notify(HookEvent::AfterToolExecute, &mut self.build_tool_result_ctx(
                        round, &call.tool_name, &result
                    )).await;
                }

                self.context.add_tool_result(call.id, result);
            }
        }
    }
}
```

---

## 10. 安全约束

### 10.1 Hook 安全边界

| 约束 | 说明 | 执行机制 |
|------|------|---------|
| 优先级保护 | 用户 Hook 不可使用 0-399 优先级范围 | HookRegistry 注册时校验 |
| 插件优先级保护 | WASM 插件 Hook 强制 >= 500 | HookRegistry 注册时校验 |
| 超时强制 | 每个 Hook 有执行超时（默认 5s） | HookPipeline 使用 tokio::timeout |
| Critical 风险不可绕过 | Hook 无法降低 Critical 风险等级 | SecurityCheckHook 内部保护 |
| 内置 Hook 不可注销 | Builtin 来源的 Hook 只能禁用不能移除 | HookRegistry 注销时检查 |
| WASM Hook 沙箱隔离 | WASM Hook 在独立的 wasmtime Store 中执行 | WasmPluginHook 实现 |
| Hook 间状态隔离 | Hook 只能通过 ctx.metadata 传递信息 | HookContext 设计保证 |
| Shell Hook 权限继承 | Shell Hook 继承工作区的安全策略 | ShellHook 环境变量过滤 |

### 10.2 Shell Hook 安全限制

配置式 Shell Hook 执行时受以下约束：

```rust
/// Shell Hook 安全检查（在执行前验证）
fn validate_shell_hook(config: &ShellHookConfig) -> Result<(), HookConfigError> {
    // 1. 超时上限 30s
    if config.timeout_ms > 30_000 {
        return Err(HookConfigError::TimeoutExceeded {
            max: 30_000,
            actual: config.timeout_ms,
        });
    }

    // 2. 禁止使用 sudo
    if config.command.contains("sudo ") {
        return Err(HookConfigError::ForbiddenCommand {
            command: config.command.clone(),
            reason: "Shell Hook 不可使用 sudo".into(),
        });
    }

    // 3. 优先级范围校验
    if config.priority < 400 || config.priority > 499 {
        return Err(HookConfigError::InvalidPriority {
            min: 400,
            max: 499,
            actual: config.priority,
        });
    }

    Ok(())
}
```

### 10.3 WASM Hook 安全限制

- WASM Hook 只能读取 `HookContext` 的序列化副本，不能直接访问宿主内存
- WASM Hook 修改 payload 需要通过返回值传回，宿主侧验证后才应用
- WASM Hook 的 CPU 燃料限制独立于插件主函数的燃料配额
- WASM Hook 不能注册优先级 < 500 的 Hook

---

## 11. 调试与可观测性

### 11.1 Hook 执行追踪

HookPipeline 在 `trace_execution = true` 时记录每个 Hook 的执行详情到内存环形缓冲区（最近 1000 条），前端可通过 Tauri Command 查询。

```text
Hook 执行追踪示例（开发者面板 → Hooks 选项卡）

┌──────────────────────────────────────────────────────────────────┐
│ Hook 执行日志                                    [清空] [暂停]  │
├──────────────────────────────────────────────────────────────────┤
│ 14:30:00.123  before_tool_execute                               │
│   ├─ builtin:observability    continue   0.02ms                │
│   ├─ builtin:security_check   continue   1.23ms                │
│   └─ log_tool_calls           continue   12.5ms  (shell)       │
│                                                                  │
│ 14:30:00.136  after_tool_execute                                │
│   ├─ builtin:observability    continue   0.01ms                │
│   ├─ builtin:prompt_guard     continue   0.85ms                │
│   ├─ builtin:audit_log        continue   2.10ms                │
│   └─ log_tool_calls           continue   8.70ms  (shell)       │
│                                                                  │
│ 14:30:02.450  before_llm_call                                   │
│   ├─ builtin:observability    continue   0.02ms                │
│   ├─ builtin:privacy_filter   continue   3.45ms  (2 redacted)  │
│   └─ builtin:budget_check     continue   0.50ms                │
└──────────────────────────────────────────────────────────────────┘
```

### 11.2 Dry-run 模式

支持在不实际执行 Hook 的情况下模拟管线执行，用于验证 Hook 配置：

```rust
impl HookPipeline {
    /// Dry-run：模拟执行所有 Hook，返回预期执行计划
    /// 不实际调用 Hook::execute()，仅列出会被执行的 Hook 及其顺序
    pub fn dry_run(&self, event: &HookEvent) -> Vec<HookDryRunEntry> {
        let hooks = self.registry.hooks_for_event(event);
        hooks.iter().map(|h| HookDryRunEntry {
            name: h.name().to_string(),
            priority: h.priority(),
            enabled: h.enabled(),
            description: h.description().to_string(),
        }).collect()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HookDryRunEntry {
    pub name: String,
    pub priority: u32,
    pub enabled: bool,
    pub description: String,
}
```

### 11.3 Hook 启停控制

用户可通过设置页面或 Tauri Command 动态启停单个 Hook，无需重启应用：

```rust
// 启停内置 Hook 的约束：
// - 内置 Hook 可以禁用，但始终保持注册状态
// - 禁用 security_check Hook 时弹出安全警告
// - 禁用 observability Hook 后，Span 树不再生成
// - budget_check Hook 禁用后，预算不再检查（高风险操作）
```

---

## 12. Tauri Commands

```rust
// apps/desktop/src-tauri/src/commands/hook_commands.rs

/// 列出所有已注册的 Hook
#[tauri::command]
pub async fn list_hooks(
    state: State<'_, AppState>,
) -> Result<Vec<HookInfo>, AppError> {
    Ok(state.hook_registry.list_all())
}

/// 启用/禁用指定 Hook
#[tauri::command]
pub async fn set_hook_enabled(
    state: State<'_, AppState>,
    hook_name: String,
    enabled: bool,
) -> Result<(), AppError> {
    // 禁用安全 Hook 时发出警告
    if !enabled && hook_name.starts_with("builtin:security") {
        state.emitter.emit("security_warning", serde_json::json!({
            "message": format!("正在禁用安全 Hook: {}", hook_name),
            "severity": "high",
        })).await.ok();
    }

    state.hook_registry.set_enabled(&hook_name, enabled)
        .map_err(|e| AppError::Internal(e.to_string()))
}

/// 测试 Hook 配置（dry-run）
#[tauri::command]
pub async fn test_hook(
    state: State<'_, AppState>,
    event: String,
) -> Result<Vec<HookDryRunEntry>, AppError> {
    let hook_event: HookEvent = serde_json::from_str(&format!("\"{}\"", event))
        .map_err(|e| AppError::Validation(format!("无效事件: {}", e)))?;
    Ok(state.hook_pipeline.dry_run(&hook_event))
}

/// 获取 Hook 执行日志
#[tauri::command]
pub async fn get_hook_execution_log(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<HookExecutionRecord>, AppError> {
    Ok(state.hook_pipeline.recent_execution_log(
        limit.unwrap_or(100) as usize
    ))
}

/// 清空 Hook 执行日志
#[tauri::command]
pub async fn clear_hook_execution_log(
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state.hook_pipeline.clear_execution_log();
    Ok(())
}

/// 重新加载 Hook 配置（从 hooks.toml）
#[tauri::command]
pub async fn reload_hook_config(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<u32, AppError> {
    let workspace = state.workspace_manager.get(&workspace_id)
        .ok_or(AppError::NotFound("workspace".into()))?;

    let config = load_hook_config(Some(&workspace.root_path))
        .map_err(|e| AppError::Internal(e.to_string()))?;

    // 先注销所有 Config 来源的 Hook
    let existing: Vec<String> = state.hook_registry.list_all().iter()
        .filter(|h| matches!(h.source, HookSource::Config))
        .map(|h| h.name.clone())
        .collect();
    for name in &existing {
        state.hook_registry.unregister(name).ok();
    }

    // 注册新配置的 Hook
    let mut count = 0;
    for hook_config in config.hooks {
        if let Err(e) = validate_shell_hook(&hook_config) {
            tracing::warn!("Hook 配置校验失败: {}: {}", hook_config.name, e);
            continue;
        }
        let hook = Arc::new(ShellHook { config: hook_config });
        if let Ok(()) = state.hook_registry.register(hook, HookSource::Config) {
            count += 1;
        }
    }

    // 应用内置 Hook 覆盖
    for (name, override_config) in &config.builtin_overrides {
        if let Some(enabled) = override_config.enabled {
            state.hook_registry.set_enabled(name, enabled).ok();
        }
    }

    Ok(count)
}
```

AppState 扩展和 Command 注册：

```rust
// apps/desktop/src-tauri/src/state.rs（新增字段）

pub struct AppState {
    // ... 已有字段 ...
    pub hook_registry: Arc<HookRegistry>,
    pub hook_pipeline: Arc<HookPipeline>,
}

// apps/desktop/src-tauri/src/main.rs（注册 commands）

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // ... 已有 commands ...
            list_hooks,
            set_hook_enabled,
            test_hook,
            get_hook_execution_log,
            clear_hook_execution_log,
            reload_hook_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

---

## 13. 设计约束

### 13.1 性能预算

Hook 系统的性能开销必须控制在可接受范围内：

| 组件 | 开销预算 | 实现保证 |
|------|---------|---------|
| HookRegistry 查询 | < 0.1ms | DashMap 无锁读 + 预建索引缓存 |
| 空管线执行（无 Hook 注册） | < 0.01ms | 空 Vec 检查后立即返回 |
| 内置 Hook 管线（6 个 Hook） | < 5ms/事件 | 异步执行 + 5s 超时保护 |
| Shell Hook 执行 | < 30s/次 | tokio::timeout 强制超时 |
| 执行日志记录 | < 0.05ms | 内存环形缓冲区，无 I/O |

### 13.2 内存限制

| 资源 | 限制 |
|------|------|
| Hook 执行日志（内存） | 最多 1000 条记录，环形覆盖 |
| HookRegistry 注册上限 | 最多 100 个 Hook |
| HookContext metadata | 单个 Hook 最多写入 10 个 key |
| Shell Hook stdout 截断 | 最多 64KB |

### 13.3 不变量

1. **内置 Hook 始终存在**：注册后不可移除，只能启停
2. **优先级单调性**：同一事件的 Hook 严格按优先级排序，不可打乱
3. **事件不可创建**：HookEvent 是闭合枚举，用户不可自定义新事件类型
4. **payload 类型安全**：每个事件对应固定的 HookPayload 变体，Hook 不可改变变体类型

---

## 14. 相关文档

- [02-agent-runtime详细设计.md](02-agent-runtime详细设计.md) -- round_loop 主循环、AgentExecutor、EventEmitter（Hook 注入目标）
- [07-隐私与合规详细设计.md](../06-安全与基础设施/07-隐私与合规详细设计.md) -- PrivacyMiddleware / SensitiveFilter（重构为 PrivacyFilterHook）
- [04-安全边界详细设计.md](../06-安全与基础设施/04-安全边界详细设计.md) -- SecurityPolicy / PathGuard / DomainGuard / ShellGuard（重构为 SecurityCheckHook）
- [05-可观测性详细设计.md](../06-安全与基础设施/05-可观测性详细设计.md) -- SpanCollector / MetricsCollector 埋点（重构为 ObservabilityHook）
- [04-WASM插件沙箱API设计.md](../_v0.3规划/04-WASM插件沙箱API设计.md) -- WASM 运行时、宿主函数、PluginPool（WasmPluginHook 的执行环境）
- [01-Skills系统详细设计.md](../04-工具与扩展生态/01-Skills系统详细设计.md) -- Skill 召回注入（可作为 SkillInjectionHook 实现）
- [05-成本预算控制详细设计.md](05-成本预算控制详细设计.md) -- BudgetManager（重构为 BudgetCheckHook）
