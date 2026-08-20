# agent-core 详细设计

> 阶段：详细设计 | 状态：草稿 | 说明：核心数据类型、run_agent_turn 编排循环、Repository 层、记忆子系统

## 1. Crate 职责边界

`agent-core` 是整个项目的基础库 crate，提供以下能力：

- **核心数据类型**：`AgentContext`、`Message`、`ToolCall`、`ConversationMeta` 等，所有 crate 共享
- **Agent 纯函数**：`build_chat_request` / `parse_response` / `execute_tool_calls` — 供 runtime 循环调用的无状态构建块
- **存储访问层**：SQLite（via sqlx）的连接池初始化、迁移执行、各表的 CRUD Repository
- **记忆子系统**：噪声过滤、写入、KNN 自适应检索、软替换更新、Weibull 衰减遗忘
- **配置加载**：`providers.toml` + `~/.astro/secrets/` 的 `ProviderConfig` 构建
- **成本追踪**：`TokenUsage` 累加，写入 `cost_records` 并向前端推送 `token_usage` 事件

依赖它的 crate：`agent-providers`（调用 TextClient trait）、`agent-runtime`（并发调度）、Tauri 主进程。

---

## 2. 目录结构

```text
crates/agent-core/
├── Cargo.toml
├── migrations/
│   ├── 0001_init.sql
│   ├── 0002_vec_tables.sql
│   ├── 0003_media_artifacts.sql
│   ├── 0004_memory_versioning.sql
│   └── 0005_fts.sql
└── src/
    ├── lib.rs                  # pub use 重导出
    ├── types/
    │   ├── mod.rs
    │   ├── message.rs          # Message, Role, ContentPart
    │   ├── tool_call.rs        # ToolCall, ToolDefinition, ToolResult
    │   ├── context.rs          # AgentContext
    │   └── config.rs           # ProviderConfig, WorkspaceConfig
    ├── storage/
    │   ├── mod.rs              # open() — 连接池 + 迁移
    │   ├── conversation.rs     # ConversationRepo
    │   ├── message.rs          # MessageRepo
    │   ├── memory.rs           # MemoryRepo（含软替换、版本查询）
    │   ├── media.rs            # MediaTaskRepo
    │   └── artifacts.rs        # AiArtifactRepo
    ├── memory/
    │   ├── mod.rs
    │   ├── filter.rs           # 噪声过滤（pre-write gate）
    │   ├── embed.rs            # 嵌入向量生成与写入 embeddings 表
    │   ├── retrieve.rs         # 自适应检索（触发词 KNN + FTS5 BM25）
    │   ├── update.rs           # 软替换更新（superseded_by 链）
    │   └── forget.rs           # Weibull 衰减、soft_expire
    ├── agent/
    │   ├── mod.rs
    │   ├── helpers.rs          # build_chat_request / parse_response / execute_tool_calls
    │   └── cost.rs             # CostTracker
    └── config/
        ├── mod.rs
        └── loader.rs           # providers.toml + secrets/ 加载
```

---

## 3. 核心数据类型

### 3.1 Message

```rust
pub struct Message {
    pub id: String,                          // UUID
    pub role: Role,                          // User | Assistant | Tool | System
    pub content: Vec<ContentPart>,           // 多模态内容块
    pub reasoning_content: Option<String>,  // DeepSeek / MiniMax 思考内容
    pub tool_call_id: Option<String>,        // tool 结果消息时关联的调用 ID
    pub tool_name: Option<String>,
    pub created_at: i64,                     // Unix ms
}

/// 统一多模态内容块（定义在 agent-types crate）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    Text { text: String },
    Image(ImageContent),
    Audio(AudioContent),
    Video(VideoContent),
    ToolCall(ToolCallContent),
    ToolResult(ToolResultContent),
}

pub struct ImageContent {
    pub data: MediaData,
    pub mime_type: Option<String>,
}

pub struct AudioContent {
    pub data: MediaData,
    pub format: Option<String>,    // "mp3", "wav", "opus"
    pub duration_secs: Option<f32>,
}

pub struct VideoContent {
    pub data: MediaData,
    pub duration_secs: Option<f32>,
}

pub struct ToolCallContent {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

pub struct ToolResultContent {
    pub call_id: String,
    pub output: Vec<ToolContent>,  // 复用工具系统的 ToolContent
    pub is_error: bool,
}

/// 媒体数据的三种形式
pub enum MediaData {
    Url(String),
    Base64 { data: String, mime_type: String },
    Path(PathBuf),   // 本地文件延迟读取
}
```

> **统一定义**：此定义是全项目唯一的 `ContentPart` 版本，位于 `agent-types` crate。所有 crate（agent-core/agent-providers/agent-runtime）通过 `use agent_types::ContentPart` 引用。

### 3.2 AgentSession + TurnContext（原 AgentContext 拆分）

> **架构决策**：原 `AgentContext` 已拆分为两层，解决 God Object 问题。详见 `03-Crate结构.md`。

**AgentSession**（长生命周期，位于 agent-runtime）：持有注册表引用，整个会话复用。

```rust
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
```

**TurnContext**（短生命周期，位于 agent-types）：每轮构建，包含当前轮次的动态数据。

```rust
pub struct TurnContext {
    pub conversation_id: String,
    pub model: String,
    pub system_prompt: String,
    pub messages: Vec<Message>,
    pub available_tools: Vec<ToolDefinition>,  // BM25 动态筛选后的本轮可用工具
    pub max_tool_rounds: u32,                   // 默认 25
    pub depth: u8,                              // 子 Agent 深度（0=主 Agent）
    pub round: u32,                             // 当前轮次计数
}
```

`AgentExecutor` 持有 `AgentSession`，每次 `round_loop` 迭代从 session 构建临时 `TurnContext`。

### 3.3 ToolCall / ToolResult

```rust
pub struct ToolCall {
    pub id: String,           // 随机 UUID，唯一标识本次调用
    pub name: String,
    pub arguments: Value,     // JSON 参数
}

/// 工具执行结果（定义在 agent-types crate）
pub struct ToolResult {
    pub call_id: String,            // 对应 ToolCall.id
    pub name: String,               // 工具名称
    pub output: Vec<ToolContent>,   // 结构化输出（Text/Image/Error 等）
    pub is_error: bool,
    pub duration_ms: Option<u64>,
}
```

> **统一命名**：`call_id` 统一命名（不使用 `tool_call_id`），`output` 统一使用 `Vec<ToolContent>`（不使用 `String`），与工具系统的 `ToolOutput.contents` 保持一致。

---

## 4. Agent 执行循环

> **架构决策**：Agent 主循环 `round_loop` 统一定义在 `agent-runtime` crate 的 `AgentExecutor` 中（见 `02-agent-runtime详细设计.md`）。`agent-core` 不包含循环编排逻辑，仅提供以下纯函数供 runtime 调用：

```rust
/// 构建 LLM 请求（组装 system prompt + message history + tool schemas）
pub fn build_chat_request(
    context: &TurnContext,
    tools: &[ToolDefinition],
) -> ChatRequest { ... }

### 执行计划提交（submit_plan 工具）

> **架构决策**：放弃 `<agent_plan>` XML 标签的文本解析方案（存在代码块误触发、UTF-8 偏移 panic、嵌套标签等边缘问题），改为通过 `submit_plan` 内置工具提交计划。LLM 使用结构化的 tool_call 提交计划，消除了文本解析的脆弱性。

`submit_plan` 工具定义：

```json
{
    "name": "submit_plan",
    "description": "当任务预计需要 3 步以上工具调用或涉及不可逆操作时，先提交执行计划等待用户确认。",
    "input_schema": {
        "type": "object",
        "properties": {
            "plan": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "step": { "type": "integer" },
                        "action": { "type": "string" },
                        "tool": { "type": "string" },
                        "risk": { "type": "string", "enum": ["low", "medium", "high"] }
                    }
                }
            },
            "reasoning": { "type": "string" }
        },
        "required": ["plan"]
    }
}
```

**互斥规则**：`submit_plan` 出现在 tool_calls 中时，round_loop 暂停执行所有其他 tool_calls，等待用户确认计划。确认后在下一轮由 LLM 重新生成实际的 tool_calls。

`parse_response` 更新：
```rust
pub fn parse_response(response: ChatResponse) -> ParsedResponse {
    let has_plan = response.tool_calls.iter().any(|tc| tc.name == "submit_plan");
    ParsedResponse {
        text: response.text,
        reasoning: response.reasoning_content,
        tool_calls: if has_plan {
            // 仅保留 submit_plan，过滤其他 tool_calls
            response.tool_calls.into_iter().filter(|tc| tc.name == "submit_plan").collect()
        } else {
            response.tool_calls
        },
        has_plan,
    }
}
```

```rust
/// 并发执行一批工具调用（无依赖关系的工具并发，有依赖的串行）
pub async fn execute_tool_calls(
    calls: Vec<ToolCall>,
    registry: &ToolRegistry,
    guard: &HumanGuard,
) -> Vec<ToolResult> { ... }

pub struct ParsedResponse {
    pub text: Option<String>,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub has_plan: bool,
}
```

> 这些函数被 `AgentExecutor::round_loop()` 按顺序调用，形成完整的 Agent 执行循环。循环控制（max_tool_rounds 守卫、暂停感知、Pending 队列注入、自适应规划）由 agent-runtime 负责。
>
> `run_agent_turn` 是纯编排函数，不包含运行时关注点（审批、预算、规划）。在生产环境中由 `agent-runtime` 的 `AgentExecutor::round_loop()` 调用，后者负责注入运行时上下文。

---

## 5. 存储层设计

### 5.1 连接池初始化

```rust
// crates/agent-core/src/storage/mod.rs
pub async fn open(path: &Path, passphrase: &str) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .before_acquire(|conn, _| Box::pin(async move {
            // sqlite-vec C 扩展必须在每条连接上加载
            unsafe { sqlite3_vec_init(conn.as_raw_handle().as_ptr()); }
            Ok(true)
        }))
        .connect_with(options)
        .await?;

    // 编译期内嵌 migrations/，按版本号自动升序执行
    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}
```

### 5.2 Repository 模式

每张表对应一个 Repository struct，持有 `SqlitePool` 引用：

```rust
pub struct MemoryRepo(SqlitePool);

impl MemoryRepo {
    /// 写入新记忆，若 key 已有活跃版本则自动软替换
    pub async fn upsert(&self, entry: &NewMemoryEntry) -> anyhow::Result<String> { ... }

    /// 软删除（设置 valid_to = now()）
    pub async fn soft_expire(&self, id: &str) -> anyhow::Result<()> { ... }

    /// 查询所有活跃版本（valid_to IS NULL）
    pub async fn list_active(&self, workspace_id: &str) -> anyhow::Result<Vec<MemoryEntry>> { ... }

    /// 按 workspace + category 批量查 Weibull 分数偏低的条目（遗忘调度器调用）
    pub async fn scan_for_forgetting(
        &self, workspace_id: &str, threshold: f64,
    ) -> anyhow::Result<Vec<MemoryEntry>> { ... }
}
```

---

## 6. 配置加载

### providers.toml 结构

```toml
[providers.anthropic]
enabled = true
models = ["claude-sonnet-4-5", "claude-opus-4-5"]

[providers.minimax]
enabled = true
models = ["MiniMax-Text-01", "MiniMax-M1"]

[providers.openai]
enabled = false
models = ["gpt-4o"]
```

### 加载逻辑

```rust
pub fn load_provider_configs(
    providers_toml: &Path,
    secrets_dir: &Path,     // ~/.astro/secrets/
) -> anyhow::Result<Vec<ProviderConfig>> {
    let raw: TomlProviders = toml::from_str(&fs::read_to_string(providers_toml)?)?;

    raw.providers.into_iter().map(|(name, cfg)| {
        let key_path = secrets_dir.join(format!("{}.key", name));
        ProviderConfig {
            name,
            enabled: cfg.enabled,
            models: cfg.models,
            has_key: key_path.exists(),   // 只传布尔值给前端，密钥不出进程
        }
    }).collect()
}
```

密钥读取仅在 `ProviderRegistry::build_client` 时发生，且只在 Rust 进程内完成 HTTP 请求头注入，永不序列化到前端。

---

## 7. 成本追踪

```rust
pub struct CostTracker {
    pool: SqlitePool,
    workspace_id: String,
    conversation_id: String,
}

impl CostTracker {
    pub async fn record(&self, model: &str, usage: &TokenUsage) -> anyhow::Result<()> {
        let cost_usd = calculate_cost(model, usage);
        sqlx::query!(
            "INSERT INTO cost_records (id, workspace_id, conversation_id, model,
             token_input, token_output, cost_usd)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            uuid(), self.workspace_id, self.conversation_id,
            model, usage.input_tokens, usage.output_tokens, cost_usd
        ).execute(&self.pool).await?;
        Ok(())
    }
}
```

---

## 8. AgentEvent 枚举

`AgentExecutor::round_loop`（agent-runtime）通过 `mpsc::Sender<AgentEvent>` 向 Tauri 层推送事件，Tauri 层再通过 `app_handle.emit` 转为前端 Tauri 事件。以下枚举定义在 agent-core 中供各 crate 共享：

```rust
pub enum AgentEvent {
    TokenChunk(String),                  // 正文增量
    ThinkingDelta(String),               // thinking 增量（DeepSeek/MiniMax）
    ToolCall(ToolCall),                  // 工具调用开始
    ToolResult(ToolResult),              // 工具执行完成
    ApprovalRequest(ApprovalRequest),    // 需要人工确认（L2/L3 风险）
    TtsChunk(Vec<u8>),                   // TTS 音频块（PCM/MP3 片段）
    MediaTaskUpdate(MediaTaskEvent),     // 媒体任务状态变更
    Done,                                // 本轮结束
    Error(String),                       // 不可恢复错误
}
```
