# Crate 结构详解

> 阶段：系统设计 | 状态：定稿 | 说明：各 Rust crate 详细结构与核心 trait 定义
>
> 本文档已根据 04-详细设计阶段 的所有详细设计文档进行同步更新（2026-08-10）。

## Cargo Workspace 根配置

```toml
[workspace]
members = [
    "crates/agent-types",
    "crates/agent-core",
    "crates/agent-providers",
    "crates/agent-runtime",
    "crates/agent-mcp-server",
    "crates/agent-evals",
    "apps/desktop/src-tauri",
]
resolver = "2"

[workspace.dependencies]
tokio        = { version = "1", features = ["full"] }
serde        = { version = "1", features = ["derive"] }
serde_json   = "1"
anyhow       = "1"
thiserror    = "2"
async-trait  = "0.1"
tracing      = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
uuid         = { version = "1", features = ["v4", "serde"] }
chrono       = { version = "0.4", features = ["serde"] }
reqwest      = { version = "0.12", features = ["json", "stream", "multipart"] }
tokio-stream = "0.1"
bytes        = "1"
rhai         = "1"
```

---

## agent-types（共享类型 crate）

> **架构决策**：为打破 agent-core 与 agent-providers 之间的潜在循环依赖，提取所有跨 crate 共享的类型和 trait 定义到独立的 `agent-types` crate。该 crate 是整个项目的最底层依赖，零业务逻辑。

**目录结构**：

```text
crates/agent-types/
├── src/
│   ├── lib.rs
│   ├── message.rs          # Message, ContentPart, MessageRole
│   ├── tool.rs             # Tool trait, ToolInput, ToolOutput, ToolError, ToolCall, ToolResult
│   ├── provider.rs         # TextClient, EmbeddingClient, TtsClient, AsrClient, ImageClient, VideoClient, MusicClient traits
│   ├── skill.rs            # Skill trait, SkillManifest
│   ├── media.rs            # MediaContent, MediaData, ImageSize, VideoGenStatus, MusicPollResult
│   ├── permission.rs       # Permission(5 核心变体: ReadFile/WriteFile/Network/Execute/SpawnAgent，v0.3 扩展至 12), PermissionSet, RiskLevel(Low/Medium/High/Critical 四级，不含 None)
│   ├── error.rs            # AgentError, ProviderError(11变体, 含 is_retryable()/should_failover()), ToolError(8变体)
│   ├── cost.rs             # CostRecord
│   ├── context.rs          # TurnContext（短生命周期上下文，见 AgentSession/TurnContext 拆分）
│   ├── event.rs            # AgentEvent enum（TokenChunk/ThinkingDelta/ToolCall/ToolResult/ApprovalRequest/Done/Error）
│   ├── hook.rs             # HookEvent enum（18种事件），HookContext, HookPayload, HookResult
│   ├── budget.rs           # BudgetConfig, WorkspaceBudgetConfig, OverflowPolicy
│   └── lifecycle.rs        # AgentLifecycle enum（Uninitialized/Initializing/Ready/Running{conversation_id}/Paused{reason}/Terminating/Terminated{reason}）
├── Cargo.toml
```

**依赖方向**（严格单向，禁止循环）：

```text
agent-types          ← 零依赖的纯类型层
    ↑
agent-providers      ← 实现 Provider traits（依赖 agent-types）
agent-core           ← 内置工具 + 存储层（依赖 agent-types）
    ↑
agent-runtime        ← 执行循环 + HumanGuard（依赖 core + providers + types）
agent-mcp-server     ← MCP 暴露（依赖 runtime + types）
agent-evals          ← 评估套件（依赖 runtime + types）
```

---

## agent-core

**职责边界**：内置工具实现（`tools/builtin/`）、存储访问层、记忆子系统、Hook 引擎、安全策略、预算管理、可观测性及知识库。核心 trait 与共享类型已提取到 `agent-types`，agent-core 依赖 `agent-types` 获取 `Tool`/`Skill`/`Message` 等类型定义，不再直接定义跨 crate 共享的 trait。不包含 Agent 执行主循环等编排逻辑，禁止依赖 agent-runtime 或 agent-providers。
Agent 主循环（`AgentExecutor::run()`）位于 **agent-runtime**，agent-core 提供内置工具实现与存储层，保证依赖倒置。

```text
crates/agent-core/src/
├── lib.rs
├── error.rs               # CoreError 统一错误枚举
├── agent/
│   ├── mod.rs             # AgentTrait 接口定义
│   └── state.rs           # AgentState 枚举（9变体，见下文）
├── skills/
│   ├── mod.rs             # SkillTrait + SkillManifest
│   ├── manifest.rs        # SkillManifest 解析与验证
│   ├── registry.rs        # SkillRegistry + BM25 动态筛选
│   ├── loader.rs          # 多级发现：内置 → 用户 → 工作区 → 项目
│   ├── context_inject.rs  # !cmd 预处理与上下文注入
│   ├── substitution.rs    # $ARGUMENTS 变量替换
│   ├── invocation.rs      # Skill 调用执行逻辑
│   ├── lifecycle.rs       # Skill 生命周期管理
│   ├── overrides.rs       # Skill 覆盖与优先级
│   ├── permissions.rs     # Skill 权限声明与校验
│   ├── watcher.rs         # 文件变更检测与热加载
│   └── evolution.rs       # Skill 自动演进（与自我进化子系统协同）
├── tools/
│   ├── mod.rs             # Tool trait + ToolRegistry + ToolInput/ToolOutput/ToolError
│   ├── builtin/           # 内置原子工具（实现 Tool trait）
│   │   ├── file.rs        # file_read / file_write / file_edit / ...
│   │   ├── dir.rs         # dir_list / dir_search
│   │   ├── shell.rs       # shell_exec / shell_bg / process_kill
│   │   ├── network.rs     # http_request（Medium 风险）/ web_search
│   │   ├── code.rs        # python_exec / rhai_exec / wasm_exec
│   │   ├── memory.rs      # memory_search / memory_manage / memory_snapshot / memory_purge
│   │   ├── skill.rs       # skill_search / skill_run / skill_manage
│   │   ├── knowledge.rs   # knowledge_search / knowledge_ingest / knowledge_manage
│   │   ├── agent.rs       # delegate_task（统一名称）/ agent_pause
│   │   └── system.rs      # system_info
│   └── mcp/               # MCP 工具适配层
│       ├── mod.rs
│       ├── client.rs      # MCP client 实现
│       └── transport.rs    # 统一传输层（基于 rmcp crate，仅支持 STDIO / Streamable HTTP）
│       └── bridge.rs      # McpToolBridge → Tool trait 适配（风险等级可配置：per-server + per-tool override）
│   # 注：浏览器工具（goto/fetch/screenshot/click/type）通过 MCP Server（mcp-server-puppeteer）提供，
│   #     不内置。避免捆绑 Chromium（100MB+ 包体积），且浏览器进程天然隔离更安全。
├── hooks/                 # Hook 引擎
│   ├── mod.rs
│   ├── event.rs           # HookEvent 定义与匹配
│   ├── registry.rs        # HookRegistry — Hook 注册与查找
│   ├── pipeline.rs        # HookPipeline — 事件触发与链式执行
│   ├── config.rs          # Hook 配置解析
│   ├── shell_hook.rs      # Shell Hook 执行器
│   └── builtin/           # 内置 Hook 实现
│       ├── privacy.rs     # 隐私保护 Hook
│       ├── security.rs    # 安全审计 Hook
│       ├── observability.rs # 可观测性 Hook
│       ├── budget.rs      # 预算检查 Hook
│       ├── audit.rs       # 审计日志 Hook
│       └── prompt_guard.rs # Prompt 注入防护 Hook
├── security/              # 安全策略引擎
│   ├── mod.rs
│   ├── policy.rs          # SecurityPolicy — 安全策略定义与评估
│   ├── path_guard.rs      # 路径访问守卫
│   ├── domain_guard.rs    # 域名白名单守卫
│   ├── shell_guard.rs     # Shell 命令过滤守卫
│   ├── prompt_guard.rs    # Prompt 注入检测
│   ├── rate_limiter.rs    # 速率限制器
│   ├── audit.rs           # 安全审计日志
│   └── config.rs          # 安全配置
├── budget/                # 预算管理
│   ├── mod.rs
│   ├── manager.rs         # BudgetManager — 预算分配与跟踪
│   ├── pricing.rs         # ModelPricingTable — 模型定价表
│   ├── estimator.rs       # CostEstimator — 成本预估
│   ├── degradation.rs     # 预算耗尽时的降级策略
│   └── report.rs          # CostReport — 成本报告生成
├── telemetry/             # 可观测性
│   ├── mod.rs
│   ├── logger.rs          # LogEvent — 结构化日志
│   ├── tracer.rs          # SpanTree, SpanCollector — 分布式追踪
│   ├── metrics.rs         # MetricsCollector — 指标采集
│   ├── cost.rs            # TokenUsage — Token 用量统计
│   └── privacy.rs         # 日志脱敏器
├── knowledge/             # 知识库
│   ├── mod.rs
│   ├── service.rs         # KnowledgeService — 知识库服务入口
│   ├── ingestor.rs        # KnowledgeIngestor trait — 文档摄入
│   ├── chunker.rs         # RecursiveChunker — 递归文本分块
│   ├── retriever.rs       # HybridRetriever — 混合检索（BM25 + 向量）
│   └── repo.rs            # KnowledgeRepo — 知识库存储
├── memory/
│   ├── mod.rs             # MemoryStore trait
│   ├── episodic.rs        # 情节记忆：短期对话上下文 (Vec<Message>)
│   ├── semantic.rs        # 语义记忆：sqlite-vec 向量检索
│   ├── persistent.rs      # 持久记忆：MEMORY.md / USER.md 冻结快照注入
│   ├── procedural.rs      # 程序性记忆：Skills 索引
│   └── user_model.rs      # 用户建模：LLM 推理 → USER.md 更新
├── storage/
│   ├── mod.rs
│   ├── repos/             # 存储仓库层
│   │   ├── conversation.rs # 对话持久化
│   │   ├── message.rs     # 消息存储
│   │   ├── memory.rs      # 记忆存储
│   │   ├── media.rs       # 媒体资源存储
│   │   ├── knowledge.rs   # 知识库存储
│   │   ├── usage.rs       # 用量记录
│   │   └── span.rs        # Span 追踪存储
│   └── migration/         # 数据迁移
│       ├── mod.rs         # MigrationRunner — 迁移执行引擎
│       ├── backup.rs      # 迁移前备份
│       ├── data_migrator.rs # 数据迁移器
│       └── online.rs      # 在线迁移（零停机）
├── evolution/             # 自我进化子系统（v0.3 精简版）
│   ├── mod.rs
│   ├── trace.rs           # TaskTrace 记录
│   ├── reflection.rs      # LLM 反思（任务完成后生成改进建议）
│   └── skill_extractor.rs # "保存为 Skill" — 从对话提取 SKILL.md
└── config.rs
```

### 核心 Trait

```rust
// Tool trait — 原子能力单元（权威定义，与 05-工具系统设计.md 保持一致）
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;    // JSON Schema draft-07
    fn risk_level(&self) -> RiskLevel;
    fn required_permissions(&self) -> Vec<Permission>;
    fn is_readonly(&self) -> bool;
    fn timeout_ms(&self) -> u64 { 30_000 }
    async fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError>;
}
// 完整类型定义见 05-工具系统设计.md §2

// Skill trait — 高层能力单元（可编排多个 Tool）
#[async_trait]
pub trait Skill: Send + Sync {
    fn manifest(&self) -> &SkillManifest;
    async fn execute(
        &self,
        session: &AgentSession,
        ctx: &mut TurnContext,
        input: SkillInput,
    ) -> anyhow::Result<SkillOutput>;
}
```

### Agent 状态机

```rust
/// 9 变体状态机，覆盖完整 Agent 生命周期（无关联数据，状态轻量可 Clone）
pub enum AgentState {
    Idle,                          // 空闲，等待输入
    Planning,                      // 正在规划
    WaitingForPlanConfirmation,    // 等待用户确认计划
    ExecutingTool,                 // 执行工具中
    WaitingForApproval,            // 等待人工审批
    SpawningAgent,                 // 生成子 Agent
    Paused,                        // 暂停
    Done,                          // 完成
    Failed,                        // 失败
}
```

### AgentSession + TurnContext — 两层上下文模型

> **设计理由**：将原 `AgentContext` 拆分为两层，解决了"God Object"问题：
>
> - `AgentSession` 持有 Arc 引用，创建成本低，整个会话复用
> - `TurnContext` 每轮构建，包含动态变化的消息历史和工具列表
> - `AgentExecutor` 持有 `AgentSession`，每次 `round_loop` 迭代从 session 构建临时 `TurnContext`

```rust
/// 长生命周期：整个会话期间持有，存储注册表引用
/// 位于 agent-runtime
pub struct AgentSession {
    pub session_id: Uuid,
    pub workspace_id: String,
    pub tools: Arc<ToolRegistry>,
    pub skills: Arc<SkillRegistry>,
    pub providers: Arc<ProviderRegistry>,
    pub supervisor: Arc<Mutex<Supervisor>>,
    pub memory: Arc<dyn MemoryStore>,
    pub budget: Arc<TokenBudget>,
    pub guard: Arc<HumanGuard>,
    pub span: tracing::Span,
}

/// 短生命周期：每轮构建，包含当前轮次的上下文数据
/// 位于 agent-types
pub struct TurnContext {
    pub conversation_id: String,
    pub model: String,
    pub system_prompt: String,
    pub messages: Vec<Message>,
    pub available_tools: Vec<ToolDefinition>,  // 本轮可用工具（BM25 动态筛选后）
    pub max_tool_rounds: u32,                   // 默认 25
    pub depth: u8,                              // 子 Agent 深度
    pub round: u32,                             // 当前轮次计数
}
```

---

## agent-providers

多模态、多 Provider 实现层。依赖 `agent-types` 获取 `TextClient`/`EmbeddingClient` 等 Provider trait 定义及 `Message`/`MediaContent` 等共享类型，不依赖 `agent-core`。`ProviderError` 统一枚举定义于 `agent-types`，每个变体均实现 `retryability() -> Retryability` 方法以支持上层重试决策。

```text
crates/agent-providers/src/
├── lib.rs
├── types/
│   ├── mod.rs
│   ├── message.rs          # 统一消息格式
│   ├── media.rs            # MediaContent 枚举
│   ├── embedding.rs
│   └── capabilities.rs     # Provider 能力声明
├── traits/
│   ├── mod.rs
│   ├── text.rs             # TextClient trait
│   ├── embedding.rs        # EmbeddingClient trait
│   ├── image.rs            # ImageClient trait
│   ├── audio.rs            # TtsClient / AsrClient trait
│   ├── video.rs            # VideoClient trait
│   └── music.rs            # MusicClient trait
├── registry.rs             # ProviderRegistry，运行时路由
├── failover.rs             # FailoverClient — 故障转移客户端，CircuitBreaker 熔断器，CircuitState 状态机
├── middleware/
│   └── privacy.rs          # PrivacyMiddleware — 请求/响应隐私过滤
└── providers/
    ├── anthropic/
    ├── openai/
    ├── google/
    │   ├── mod.rs
    │   ├── interactions.rs  # GoogleInteractionsClient — 对话/Agent + Lyria 3 音乐生成
    │   ├── generate.rs      # GoogleGenerateContentClient — 嵌入/RAG/批量
    │   ├── image.rs         # Imagen 3（图像生成）
    │   ├── tts.rs           # Cloud TTS Chirp3-HD（语音合成）
    │   ├── asr.rs           # Cloud STT（语音识别）
    │   ├── embedding.rs     # text-embedding-004
    │   ├── video.rs         # Veo 3.1（视频生成，8秒，LongRunning 轮询）
    │   └── music.rs         # Lyria 3（音乐生成，via Interactions API，Clip/Pro 两型）
    ├── deepseek/
    │   └── mod.rs           # DeepSeekClient — reasoning_content 回传处理
    ├── minimax/
    │   ├── mod.rs           # MiniMaxClient — reasoning_split / base_resp / max_completion_tokens
    │   ├── tts.rs           # speech-2.8-hd（语音合成，流式；支持情感标签/字幕/音色设计/声音克隆）
    │   ├── asr.rs           # asr-01（语音识别）
    │   ├── image.rs         # image-01 / image-01-live（文生图+图生图，人物主体参考，画风控制）
    │   ├── video.rs         # video-01（视频生成，Task 模式轮询）
    │   ├── music.rs         # music-3.0 / music-cover（音乐生成 + 翻唱，含歌词生成接口）
    │   └── voice.rs         # 音色设计（/v1/voice_design）+ 声音克隆上传（/v1/files/upload）
    └── ollama/
        └── mod.rs           # OllamaClient — 实现 TextClient + EmbeddingClient，本地模型推理
```

---

## agent-runtime

**职责边界**：Agent 执行主循环 + 脚本/WASM 沙箱执行器。依赖 agent-core（内置工具 + 存储）、agent-providers（LLM 调用）和 agent-types（共享类型），对应分层架构图中的 **L3 运行时编排层**。`AgentSession`（长生命周期会话上下文）定义于此 crate。

```text
crates/agent-runtime/src/
├── lib.rs
├── executor.rs             # AgentExecutor::round_loop() — Agent 主循环
├── planner.rs              # Planner — 自适应规划
├── instance.rs             # AgentInstance — 顶层Agent对象、AgentLifecycle状态机
├── pending_queue.rs        # PendingQueue — 运行中消息注入
├── context.rs              # AgentContext construction
├── retry.rs                # with_retry, IsRetryable, exponential backoff
├── event_emitter.rs        # EventEmitter trait, TauriEventEmitter
├── supervisor.rs           # Supervisor — 子Agent生命周期
├── orchestrator.rs         # SkillOrchestrator
├── session/
│   ├── mod.rs
│   ├── manager.rs          # AgentSessionManager — 并发实例管理、LRU缓存
│   └── handle.rs           # SessionHandle
├── guard/
│   ├── mod.rs
│   ├── human_guard.rs      # HumanGuard — GuardState状态机
│   ├── state.rs            # GuardState enum（Running/Pending/Paused/Takeover）
│   ├── whitelist.rs        # 白名单快速通行
│   └── sub_agent_guard.rs  # SubAgentGuard — 子Agent资源限制
├── sandbox/
│   ├── mod.rs
│   ├── wasm_sandbox.rs     # [v0.3 暂缓] Wasmtime WASM沙箱，MCP+SKILL.md 已覆盖
│   ├── rhai_sandbox.rs     # Rhai脚本沙箱
│   └── shell.rs            # Shell命令沙箱
├── background/
│   ├── mod.rs
│   ├── media_poller.rs     # MediaTaskPoller
│   ├── distiller.rs        # MemoryDistiller
│   └── memory_gc.rs        # MemoryGC — 过期记忆清理（按 TTL，非 GDPR 保留策略）
├── hooks/
│   ├── mod.rs
│   ├── wasm_hook.rs        # [v0.3 暂缓] WasmPluginHook
│   └── integration.rs      # register_builtin_hooks, round_loop集成
├── skills/
│   ├── mod.rs
│   ├── test_runner.rs      # SkillTestRunner
│   └── subagent.rs         # Skill subagent执行
├── workflow/
│   ├── mod.rs
│   ├── skill_chain.rs      # SkillChain — 顺序执行 + 条件分支（v0.3 精简版）
│   ├── trigger.rs          # 手动触发 + CronTrigger（v0.3 精简版）
│   └── compiler.rs         # compile_to_skill() — 工作流→SKILL.md
└── provider/
    ├── failover.rs         # FailoverClient
    ├── circuit_breaker.rs  # CircuitBreaker
    └── network_monitor.rs  # NetworkMonitor — 离线检测
```

---

## agent-mcp-server

将本 Agent 的能力暴露为标准 MCP server，供其他 Agent 或工具调用。

```text
crates/agent-mcp-server/src/
├── main.rs
├── server.rs           # MCP server 协议实现
└── handlers.rs         # Agent 能力 → MCP resource/tool 映射
```

---

## agent-evals

独立评估套件，衡量 Agent 质量（非单元测试）。

```text
crates/agent-evals/src/
├── main.rs              # CLI入口：astro eval run --dataset xxx.yaml
├── dataset.rs           # EvalDataset, EvalCase — YAML 格式数据集
├── runner.rs            # EvalRunner — 顺序执行
├── judge/
│   ├── mod.rs
│   ├── exact.rs         # 精确匹配
│   ├── contains.rs      # 关键词包含
│   └── regex.rs         # 正则匹配
└── report.rs            # 通过率统计 + CI 门控（pass_rate >= threshold）
```

---

## apps/desktop（Tauri 桌面应用）

Tauri 2 桌面应用壳层，通过 Tauri Command 暴露 agent-runtime 能力给前端 UI。依赖 agent-runtime 和 agent-types。

```text
apps/desktop/src-tauri/src/
├── main.rs
├── lib.rs               # Tauri plugin注册
├── state.rs             # AppState — 依赖注入容器
├── error.rs             # AppError — 顶层错误枚举
├── sidecar.rs           # OllamaSidecar管理
├── commands/
│   ├── mod.rs
│   ├── conversation.rs  # 对话CRUD
│   ├── message.rs       # 消息发送/流式
│   ├── workspace.rs     # 工作区管理
│   ├── guard.rs         # HumanGuard审批
│   ├── budget.rs        # 预算状态/设置
│   ├── skill.rs         # Skill操作
│   ├── knowledge.rs     # 知识库操作
│   ├── search.rs        # 全局搜索
│   ├── export.rs        # 导出/分享
│   ├── settings.rs      # 设置读写
│   ├── telemetry.rs     # 可观测性/指标
│   ├── security.rs      # 安全策略
│   ├── privacy.rs       # 隐私设置
│   ├── workflow.rs      # 工作流操作
│   ├── marketplace.rs   # Agent市场
│   ├── update.rs        # 应用更新
│   └── agent.rs         # Agent生命周期
└── events.rs            # Tauri事件定义
```

---

## 扩展性与可维护性设计原则

### 1. 依赖倒置：agent-types 做稳定契约层

`agent-types` 是整个系统的**稳定核心**。所有跨 crate 的 trait（`Tool`、`TextClient`、`Skill` 等）定义于此，上层 crate 仅实现 trait 而不定义新的跨 crate 接口。

**扩展新 Provider 的步骤**：
1. 在 `agent-types/provider.rs` 确认 trait 是否满足需求（通常无需改动）
2. 在 `agent-providers/providers/` 新增目录，实现 `TextClient`
3. 在 `ProviderRegistry` 注册 — 零 agent-core/runtime 改动

**扩展新 Tool 的步骤**：
1. 在 `agent-core/tools/builtin/` 新增文件，实现 `Tool` trait
2. 在 `ToolRegistry` 注册 — 零其他 crate 改动

### 2. Hook 系统做非侵入式扩展

18 种生命周期事件覆盖 Agent 执行全链路。新增横切关注点（日志、审计、限流、隐私过滤）通过实现 Hook 而非修改主循环。

**扩展新 Hook 的步骤**：
1. 在 `agent-core/hooks/builtin/` 新增文件
2. 实现 `Hook` trait，指定监听的 `HookEvent`
3. 在 `register_builtin_hooks()` 中注册 — 主循环代码不变

### 3. YAGNI 原则：延迟抽象

| 原则 | 示例 |
|------|------|
| 1 个实现不抽象 | `LocalShellBackend` 直接使用，不创建 trait |
| 2 个实现用枚举 | `ExecutionBackend::Local \| Docker` 枚举分发 |
| 3+ 个实现提取 trait | 当 SSH/E2B 加入时再提取 `ExecutionBackend` trait |

### 4. 模块边界清晰度检查清单

新增模块时确认以下问题：
- [ ] 该模块属于哪个 crate？（类型→types，业务逻辑→core，编排→runtime）
- [ ] 是否引入了新的跨 crate 依赖？（禁止 core→runtime 反向依赖）
- [ ] 是否需要新的 HookEvent？（优先复用现有事件）
- [ ] 是否需要新的 Tauri Command？（优先复用现有 command 模块）
- [ ] 该功能是 v0.1/v0.2 需要的吗？（不是→放入 _v0.3规划）

### 5. 代码组织约定

| 约定 | 说明 |
|------|------|
| 一文件一 struct | 每个主要 struct 独立文件（如 `budget/manager.rs`） |
| mod.rs 仅导出 | `mod.rs` 只做 `pub mod` 和 `pub use`，不放业务逻辑 |
| 错误就近定义 | 每个 crate 有自己的 `error.rs`，crate 边界用 `From` 转换 |
| 测试同目录 | `#[cfg(test)] mod tests` 放在同文件底部 |
| 集成测试 | `tests/` 目录用于跨模块集成测试 |
