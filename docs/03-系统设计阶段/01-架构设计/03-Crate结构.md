# Crate 结构详解

> **Agent Harness 基线（2026-09-04）**：Harness 主链由 `agent-core`、`agent-providers`、`agent-tools`、`agent-protocol`、`agent-rollout`、`agent-session`、`agent-mcp`、`agent-subagents`、`agent-sandbox`、`agent-network-proxy` 和 `agent-server` 共同组成。`AgentLoop` 是 `Session` 的兼容别名。见 [Agent Harness 总体架构](11-Agent-Harness总体架构.md)。

> 阶段：系统设计 | 状态：定稿 | 说明：各 Rust crate 详细结构与核心 trait 定义
>
> 本文档已根据实际代码库同步更新（2026-09-04）。

## Cargo Workspace 根配置

所有 Rust crate 扁平放置在 `crates/agent-*` 下（package name 保持短名），Tauri 桌面应用在 `apps/desktop/src-tauri`。共 24 个 crate + 1 个桌面应用 = 25 个 workspace 成员。

```toml
[workspace]
members = [
    "crates/agent-types",
    "crates/agent-config",
    "crates/agent-protocol",
    "crates/agent-rollout",
    "crates/agent-home",
    "crates/agent-skills",
    "crates/agent-hooks",
    "crates/agent-providers",
    "crates/agent-proto",
    "crates/agent-session",
    "crates/agent-artifacts",
    "crates/agent-usage",
    "crates/agent-mcp",
    "crates/agent-memory",
    "crates/agent-network-proxy",
    "crates/agent-sandbox",
    "crates/agent-subagents",
    "crates/agent-evolution",
    "crates/agent-cron",
    "crates/agent-workflow",
    "crates/agent-a2ui",
    "crates/agent-tools",
    "crates/agent-core",
    "crates/agent-server",
    "apps/desktop/src-tauri",
]
resolver = "2"

[workspace.dependencies]
tokio        = { version = "1", features = ["full"] }
tokio-util   = { version = "0.7" }
serde        = { version = "1", features = ["derive"] }
serde_json   = "1"
anyhow       = "1"
thiserror    = "1"
async-trait  = "0.1"
tracing      = "0.1"
schemars     = "0.8"
globset      = "0.4"
reqwest      = { version = "0.12", features = ["json", "stream", "rustls-tls", "macos-system-configuration"], default-features = false }
```

## Crate 总览表

| 路径 | package name | 职责 |
|---|---|---|
| `crates/agent-types` | `types` | 跨 crate 共享类型：`ModelTarget`、`model_tool::ToolCall`、`MediaAsset`、`ModelSpec`、`NetworkPolicy`、`PermissionProfile`、SQLite helpers、tool-spill。 |
| `crates/agent-config` | `agent-config` | 分层配置原语：`ConfigLayer`、`ConfigLayerSource`（4 级优先级）、`ConfigKeyPath`、provenance 追溯。无产品特有字段，不做文件系统发现。 |
| `crates/agent-protocol` | `protocol` | Core 领域事件协议：`Event`、`EventMsg`、`TurnItem`、`Submission`。运行时唯一事件格式。 |
| `crates/agent-rollout` | `rollout` | JSONL append-only 历史记录：`RolloutRecorder`、`PersistencePolicy`、`reconstruct` 重建。rollout 是线程历史的权威事实源。 |
| `crates/agent-home` | `home` | `~/.astro` 路径约定、日志、agent config YAML、tool-enable gates。无 SQLite。 |
| `crates/agent-skills` | `skills` | Skill 管理 — 安装、加载、注册表、摘要、备份。Skill frontmatter `astro_tools` 可 additive 开放 toolset。 |
| `crates/agent-hooks` | `hooks` | 三总线 hook 系统：Plugin（进程内同步 `PluginHookBus`）、Gateway（文件扫描外部 manifest）、Shell（config-map 异步 shell 命令）。Codex 对齐事件名。 |
| `crates/agent-providers` | `providers` | 多厂商 LLM/图像 Provider 层：Agent 使用独立 `ResponsesModel` / `ResponsesRequest`，Chat 兼容使用 `ChatCompletionModel` / `ChatCompletionRequest`；`attach_responses` 挂载 Responses 能力，二者不相互升级或降级。另含 TOML 自定义 provider、`ProviderProfile`、流式解析与 fallback。 |
| `crates/agent-proto` | `proto` | Protobuf / tonic gRPC 服务契约（backend ↔ Tauri shell）。Thread submit/resume/subscribe、ChatControl、媒体、Skill、MCP、Memory、AgentThreadChanged 等 RPC。 |
| `crates/agent-session` | `session` | `SessionStore`（`state.db` WAL SQLite，schema v22，FTS5）— 消息、会话、billing、FTS 召回、rollout 投影重建。 |
| `crates/agent-artifacts` | `artifacts` | 文件空间索引（`artifacts.db`）+ Knowledge Content DB（`knowledge.db`，FTS）。按来源注册文件，MIME 分类。 |
| `crates/agent-usage` | `usage` | 用量事件 DB（`usage.db`）、per-agent 统计、路由感知成本估算（官方定价快照 + OpenRouter API）、trace insights、eval JSONL 导出。 |
| `crates/agent-mcp` | `mcp` | MCP 客户端 — per-agent 进程级连接池（`McpHub`），工具发现与调用、OAuth 认证。工具名约定：`mcp__{server}__{tool}`。 |
| `crates/agent-memory` | `memory` | `MemoryManager` — MEMORY.md/USER.md 快照、dreaming 管道、待审批记忆队列、decision log、workspace bootstrap、权限审计。 |
| `crates/agent-network-proxy` | `network-proxy` | 受管网络代理：HTTP CONNECT 策略、per-attempt 租约、网络审批流。 |
| `crates/agent-sandbox` | `sandbox` | 沙箱权限控制：`PermissionProfile`（read-only/workspace-write/danger-full-access）、权限审计、网络策略。 |
| `crates/agent-subagents` | `subagents` | Codex V2 Agent Thread：`AgentControl`（根级共享控制器）、`AgentGraphStore`（subagents.db 图/邮箱/状态事件）、`AgentRegistry`（RAII 预留/配额）、`ActivityBus`（事件等待）、`.astro` 自定义 agent 配置。 |
| `crates/agent-evolution` | `evolution` | 自进化/学习循环：改进提议、评判、信号分析、评估集、DSPy 集成。配套 Python 包 `evolution-dspy/`。 |
| `crates/agent-cron` | `cron` | Cron job JSON 持久化、运行记录 DB（`cron_v1.db`）、ticker（每 30s，`current_thread` runtime）。 |
| `crates/agent-workflow` | `workflow` | 可视化工作流引擎：29 种节点跨 6 类（Trigger/AI/Media/FlowControl/DataProcessing/Action），DAG 执行引擎、变量解析、运行 DB。 |
| `crates/agent-a2ui` | `a2ui` | AG-UI 声明式生成式 UI 表面：22 种组件（Text、Card、Button、Image、Audio、Video、Metric、ClarifyWizard 等）、模板、校验。 |
| `crates/agent-tools` | `tools` | 全部内置工具实现（`register_all`）、`ToolRegistry`（`ToolExposure` 五级暴露 Direct/DirectModelOnly/Deferred/DeferredModelOnly/Hidden + BM25 工具搜索）、审批逻辑、HITL、schema sanitization。内部目录：`engine/`（注册表/分发/catalog/schema）、`builtin/`（shell/agents/hitl/media/memory/present）。工具域：exec_command（原 terminal）、apply_patch（Freeform 补丁工具，替代 file_ops）、write_stdin、request_permissions、code_exec、memory、skills、subagents（6 个 V2 工具）、tool_search、media 等。`FreeformToolFormat` 支持非 JSON 工具输入（Lark 语法）。 |
| `crates/agent-core` | `agent` | Agent 运行时核心：`Session` 状态机、`AstroThread` 句柄、`submission_loop` 有序提交、`SessionTask`/`ActiveTurn` 任务生命周期、`TurnContext`/`StepContext` 层级上下文、工具路由（`ToolRouter`）、压缩、HITL、hooks、prompt 组装、`git_worktree`（项目根解析与 worktree 隔离）。 |
| `crates/agent-server` | `server` | gRPC 服务端（tonic）：Thread submit/resume/subscribe RPC、`ThreadHistoryBuilder` 活跃 Turn 快照、per-connection 128 容量队列、慢消费者断连。`run_embedded()` 供 Tauri in-process 使用。 |
| `apps/desktop/src-tauri` | `astro-agent` | Tauri 2 桌面 shell。默认内嵌 backend（随机端口 `127.0.0.1:0`），单实例，系统托盘。Thread 事件流桥接。 |

---

## 依赖方向

```text
agent-types / agent-config / agent-protocol  ← 零业务逻辑的底层基础
    ↑
agent-rollout / agent-home / agent-skills / agent-hooks
agent-providers / agent-sandbox
    ↑
agent-session / agent-artifacts / agent-usage / agent-mcp / agent-memory
agent-subagents / agent-network-proxy
    ↑
agent-tools            ← 工具实现 + ToolRegistry（BM25 搜索、ToolExposure 五级暴露控制）
    ↑
agent-core             ← 运行时核心（Session、streaming、exec、prompt、compression）
    ↑
agent-server           ← gRPC 服务端
    ↑
apps/desktop/src-tauri ← Tauri 桌面壳
```

---

## agent-core

**职责边界**：Agent 运行时核心 — Session 生命周期、AstroThread 句柄、submission_loop 有序提交、SessionTask/ActiveTurn 任务生命周期、TurnContext/StepContext 层级上下文、工具路由（ToolRouter）、流式补全、上下文压缩、HITL 控制、prompt 组装、git_worktree（项目根解析与 worktree 隔离）。

主要子模块：

- **`runtime/`** — Session 生命周期、`AstroThread`、`SessionIo`、`submission_loop`、`SessionState`、`SessionServices`、`TurnContext`、`StepContext`、`ToolRouter`、`ToolRuntime`、turn lifecycle、context maintenance、recording、system prompt、`budget`（`IterationBudget` — 每 Agent Thread 独立迭代预算，`code_exec` 可 refund）、`turn_budget`（`TurnState` — 轮次与工具深度计数）
- **`tasks/`** — 可恢复任务生命周期：`SessionTask`、`ActiveTurn`、`TaskKind`、spawn/cancel/terminal 事件保证
- **`streaming/`** — 流式补全：fallback、HITL bridge、多轮 streaming、provider 抽象、tool 执行、summary
- **`exec/`** — 执行域：`AgentControlDirectory`（根级 AgentControl 进程目录）、`AgentRuntimeManager`（活跃 turn 管理）、subagents（单 turn 运行器）、dispatch（V2 6 工具分发 + 桌面控制面）、cron、background、memory review、title generation
- **`compression/`** — tool 结果压缩（原文保留，压缩视图给 provider）
- **`control/`** — HITL gate、中断状态机、schema 校验、smart approval、网络审批
- **`prompt/`** — 上下文组装、hook 集成、消息变换、prompt builder、sanitization

---

## agent-tools

**职责边界**：全部内置工具实现与注册表。工具通过 `register_all()` 批量注册到 `ToolRegistry`。`ToolExposure` 五级暴露控制：Direct/DirectModelOnly/Deferred/DeferredModelOnly/Hidden，配合 BM25 工具搜索实现延迟加载。工具域包括：exec_command（原 terminal）、apply_patch（Freeform 补丁工具，替代 file_ops）、write_stdin、request_permissions、code_exec、memory、skills、subagents（6 个 V2 工具）、tool_search、media 等。`FreeformToolFormat` 支持非 JSON 工具输入（Lark 语法）。

内部目录结构：

```text
crates/agent-tools/src/
├── lib.rs                    # register_all()、interaction_mode
├── engine/                   # 工具引擎核心
│   ├── registry.rs           # ToolRegistry — Direct schema / discovered Deferred 路由
│   ├── dispatch.rs           # 工具分发与执行
│   ├── catalog.rs            # 工具目录列表
│   ├── context.rs            # 工具执行上下文
│   ├── execution.rs          # 执行器
│   ├── executor.rs           # 异步执行器
│   ├── schema.rs             # JSON Schema 清理（sanitize_tool_schema）
│   ├── network.rs            # 网络策略检查
│   └── path_safe.rs          # 路径安全检查
├── builtin/                  # 内置工具实现
│   ├── shell/                # Shell / 系统工具
│   │   ├── terminal.rs       # exec_command（原 shell_exec）
│   │   ├── code_exec.rs      # python_exec
│   │   ├── web_fetch.rs      # web_fetch
│   │   ├── web_search.rs     # web_search
│   │   ├── jobs.rs           # 后台任务
│   │   ├── tool_search.rs    # tool_search — BM25 延迟工具发现
│   │   ├── context_remaining.rs  # get_context_remaining
│   │   ├── new_context_window.rs # new_context_window
│   │   ├── request_plugin_install.rs # request_plugin_install
│   │   └── wait_for_environment.rs   # wait_for_environment
│   ├── agents/               # 子 Agent 工具（spawn_agent 等 6 个 V2 工具）
│   ├── hitl/                 # HITL 工具（switch_mode 等）
│   ├── media/                # 媒体工具（image_gen / tts / video / music）
│   ├── memory/               # 记忆工具
│   └── present/              # 展示工具（a2ui）
└── approval.rs               # 审批逻辑
```

---

## agent-providers

多厂商 LLM/图像 Provider 层。Agent 使用独立 `ResponsesModel` / `ResponsesRequest`，Chat 兼容使用 `ChatCompletionModel` / `ChatCompletionRequest`；`attach_responses` 挂载 Responses 能力，二者不相互升级或降级。

**协议管线**（5 种 `ApiMode`）：`ChatCompletions`（OpenAI 兼容）、`Responses`（OpenAI Responses API）、`AnthropicMessages`、`Interactions`（Google Gemini）、`GeminiNative`。协议选择由 `ProviderProfile.api_mode` 默认 + `ProviderConfig.api_mode` 运行时覆盖。

**Trait 系统**：`OpenAICompatible` trait 通过声明式常量控制行为：`THINKING_FORMAT: ThinkingFormat`（4 种 thinking 线路格式：`None` / `ReasoningEffort` / `DeepSeek` / `MiniMaxAdaptive`）、`EFFORT_MAP`（推理 effort 映射表）、`SUPPORTS_RESPONSES` / `RESPONSES_STORE_FALSE` / `RESPONSES_PARALLEL_TOOLS` / `RESPONSES_REASONING_SUMMARY`。新增厂商只需设常量（2-4 行），不需要覆盖 `finalize_body()`。

**TOML 自定义 Provider**：用户在 `~/.astro/config.toml` 中声明即可接入任何 OpenAI 兼容 API，零代码。自定义 provider 统一走 Responses API，由 `ConfigDrivenResponsesModel` 实现。

**模型元数据**（三层合并）：API 厂商端点发现 → OpenRouter 模型表 enrich → 已知能力补丁（`apply_known_capability_overrides`）。合并结果持久化到 `~/.astro/cache/models.json`。

**Provider Fallback 链**：`model_targets: Vec<ModelTarget>` — primary + 最多 `MAX_MODEL_FALLBACKS` 个备用。首个 chunk 前失败则自动切换到下一目标。

**Profile 表**：`ProviderProfile` 静态表驱动（`PROFILES` 数组），每厂商一个条目。

---

## apps/desktop（Tauri 桌面应用）

Tauri 2 桌面应用壳层，默认内嵌 gRPC backend（随机端口），通过 Thread 事件流桥接前端 UI。

插件：window-vibrancy（窗口毛玻璃）、tauri-plugin-clipboard（剪贴板）、tauri-plugin-window-state（窗口状态持久化）、tauri-plugin-autostart（开机自启）。

---

## 扩展性与可维护性设计原则

### 1. 依赖倒置：agent-types 做稳定契约层

`agent-types` 是整个系统的**稳定核心**。所有跨 crate 的 trait（`Tool`、`TextClient`、`Skill` 等）定义于此，上层 crate 仅实现 trait 而不定义新的跨 crate 接口。

**扩展新 Provider 的步骤**：
1. 在 `agent-types/provider.rs` 确认 trait 是否满足需求（通常无需改动）
2. 在 `agent-providers/providers/` 新增目录，实现 `TextClient`
3. 在 `ProviderRegistry` 注册 — 零 agent-core/runtime 改动

**扩展新 Tool 的步骤**：
1. 在 `agent-tools/src/builtin/` 新增文件，实现 `Tool` trait
2. 在 `ToolRegistry` 注册 — 零其他 crate 改动

### 2. Hook 系统做非侵入式扩展

19 种 Plugin bus 生命周期事件（Codex 对齐命名 + Astro 扩展）覆盖 Agent 执行全链路：`PreLlmCall`、`PostLlmCall`、`PreToolUse`、`PostToolUse`、`PermissionRequest`、`Stop`、`Interrupt`、`PreCompact`、`PostCompact`、`SessionStart`、`SessionEnd`、`UserPromptSubmit`、`SubagentStart`、`SubagentStop`、`PreApiRequest`、`PostApiRequest`、`TransformTerminalOutput`、`TransformToolResult`、`TransformFinalLlmOutput`。事件名只接受 canonical 精确匹配。新增横切关注点（日志、审计、限流、隐私过滤）通过实现 Hook 而非修改主循环。

**扩展新 Hook 的步骤**：
1. 在 `agent-hooks/src/` 新增文件
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
