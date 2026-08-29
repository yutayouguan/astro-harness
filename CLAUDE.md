# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Astro Agent（阿童木）— 本地 AI 桌面工作站。技术栈：**Rust workspace + Tauri 2 + React/Vite**。

## Build & Development Commands

```bash
# 安装前端依赖（首次）
cd apps/desktop && npm install

# 开发模式（Tauri + 内嵌 backend + 前端热更新）
cd apps/desktop && npm run tauri dev

# 仅 Rust 编译检查（最快）
cargo check

# 单个 crate 构建
cargo build -p agent

# 全量测试
cargo test

# 单个 crate 测试
cargo test -p agent

# 单个测试函数（子字符串匹配）
cargo test -p agent completed_turn_releases_execution -- --nocapture

# 格式化 / Lint
cargo fmt --all
cargo clippy --all-targets

# TypeScript 类型检查
cd apps/desktop && npx tsc --noEmit

# 桌面应用打包（当前架构）
cd apps/desktop && npm run tauri build

# macOS 跨架构
cd apps/desktop && npm run tauri:build:arm          # aarch64-apple-darwin
cd apps/desktop && npm run tauri:build:x64          # x86_64-apple-darwin
cd apps/desktop && npm run tauri:build:universal    # universal-apple-darwin
```

**自动提交**：完成一轮可落地的代码改动后主动创建 git commit，不必等用户说「提交」。仅探索/问答、未改仓库时不提交。不要 push，除非用户明确要求。

**Worktree 收尾**：使用 git worktree 隔离完成任务后，必须主动提示用户是否合并与清理，等待用户选择后再执行。

## Workspace Crate Map

所有 Rust crate 扁平放置在 `crates/agent-*` 下（package name 保持短名），Tauri 桌面应用在 `apps/desktop/`。共 24 个 crate：

| 路径 | package name | 职责 |
|---|---|---|
| `crates/agent-core` | `agent` | Agent 运行时核心：`Session` 状态机、`AstroThread` 句柄、`submission_loop` 有序提交、`SessionTask`/`ActiveTurn` 任务生命周期、`TurnContext`/`StepContext` 层级上下文、工具路由（`ToolRouter`）、压缩、HITL、hooks、prompt 组装、`git_worktree`（项目根解析与 worktree 隔离）。 |
| `crates/agent-config` | `agent-config` | 分层配置原语：`ConfigLayer`、`ConfigLayerSource`（4 级优先级）、`ConfigKeyPath`、provenance 追溯。无产品特有字段，不做文件系统发现。 |
| `crates/agent-providers` | `providers` | 多厂商 LLM/图像 Provider 层：trait 系统（`OpenAICompatible` + `ThinkingFormat`）、数据驱动兼容、Responses API（`upgrade_to_responses` 统一注册，参见 `RESPONSES-API.md`）、TOML 自定义 provider、`ProviderProfile` 表、流式 `ChatStream`、fallback 链。支持 Google Interactions、OpenAI、Claude、DeepSeek、MiniMax、Ollama、Azure、百炼、混元等 15+ 厂商。 |
| `crates/agent-memory` | `memory` | `MemoryManager` — MEMORY.md/USER.md 快照、dreaming 管道、待审批记忆队列、decision log、workspace bootstrap、权限审计。 |
| `crates/agent-subagents` | `subagents` | Codex V2 Agent Thread：`AgentControl`（根级共享控制器）、`AgentGraphStore`（subagents.db 图/邮箱/状态事件）、`AgentRegistry`（RAII 预留/配额）、`ActivityBus`（事件等待）、`.astro` 自定义 agent 配置。 |
| `crates/agent-evolution` | `evolution` | 自进化/学习循环：改进提议、评判、信号分析、评估集、DSPy 集成。配套 Python 包 `evolution-dspy/`。 |
| `crates/agent-home` | `home` | `~/.astro` 路径约定、日志、agent config YAML、tool-enable gates。无 SQLite。 |
| `crates/agent-skills` | `skills` | Skill 管理 — 安装、加载、注册表、摘要、备份。Skill frontmatter `astro_tools` 可 additive 开放 toolset。 |
| `crates/agent-tools` | `tools` | 全部内置工具实现（`register_all`）、`ToolRegistry`（`ToolExposure` 六级暴露 Direct/DirectModelOnly/Deferred/DeferredModelOnly/CodeModeOnly/Hidden + BM25 工具搜索）、审批逻辑、HITL、schema sanitization。内部目录：`engine/`（注册表/分发/catalog/schema）、`builtin/`（shell/agents/hitl/media/memory/present）。工具域：exec_command（原 terminal）、apply_patch（Freeform 补丁工具，替代 file_ops）、write_stdin、request_permissions、code_exec、memory、skills、subagents（6 个 V2 工具）、tool_search、media 等。`FreeformToolFormat` 支持非 JSON 工具输入（Lark 语法）。 |
| `crates/agent-a2ui` | `a2ui` | AG-UI 声明式生成式 UI 表面：22 种组件（Text、Card、Button、Image、Audio、Video、Metric、ClarifyWizard 等）、模板、校验。 |
| `crates/agent-server` | `server` | gRPC 服务端（tonic）：Thread submit/resume/subscribe RPC、`ThreadHistoryBuilder` 活跃 Turn 快照、per-connection 128 容量队列、慢消费者断连。`run_embedded()` 供 Tauri in-process 使用。 |
| `crates/agent-types` | `types` | 跨 crate 共享类型：`Message`、`Role`、`ToolCall`、`MediaAsset`、`ChatTarget`、`ModelSpec`、`NetworkPolicy`、`PermissionProfile`、SQLite helpers、tool-spill。 |
| `crates/agent-proto` | `proto` | Protobuf / tonic gRPC 服务契约（backend ↔ Tauri shell）。Thread submit/resume/subscribe、ChatControl、媒体、Skill、MCP、Memory、AgentThreadChanged 等 RPC。 |
| `crates/agent-protocol` | `protocol` | Core 领域事件协议：`Event`、`EventMsg`、`TurnItem`、`Submission`。运行时唯一事件格式。 |
| `crates/agent-rollout` | `rollout` | JSONL append-only 历史记录：`RolloutRecorder`、`PersistencePolicy`、`reconstruct` 重建。rollout 是线程历史的权威事实源。 |
| `crates/agent-session` | `session` | `SessionStore`（`state.db` WAL SQLite，schema v23，FTS5）— 消息、会话、billing、FTS 召回、rollout 投影重建。 |
| `crates/agent-artifacts` | `artifacts` | 文件空间索引（`artifacts.db`）+ Knowledge Content DB（`knowledge.db`，FTS）。按来源注册文件，MIME 分类。 |
| `crates/agent-usage` | `usage` | 用量事件 DB（`usage.db`）、per-agent 统计、路由感知成本估算（官方定价快照 + OpenRouter API）、trace insights、eval JSONL 导出。 |
| `crates/agent-cron` | `cron` | Cron job JSON 持久化、运行记录 DB（`cron.db`）、ticker（每 30s，`current_thread` runtime）。 |
| `crates/agent-workflow` | `workflow` | 可视化工作流引擎：29 种节点跨 6 类（Trigger/AI/Media/FlowControl/DataProcessing/Action），DAG 执行引擎、变量解析、运行 DB。 |
| `crates/agent-mcp` | `mcp` | MCP 客户端 — per-agent 进程级连接池（`McpHub`），工具发现与调用、OAuth 认证。工具名约定：`mcp__{server}__{tool}`。 |
| `crates/agent-hooks` | `hooks` | 三总线 hook 系统：Plugin（进程内同步 `PluginHookBus`）、Gateway（文件扫描外部 manifest）、Shell（config-map 异步 shell 命令）。Codex 对齐事件名。 |
| `crates/agent-sandbox` | `sandbox` | 沙箱权限控制：`PermissionProfile`（read-only/workspace-write/danger-full-access）、权限审计、网络策略。 |
| `crates/agent-network-proxy` | `network-proxy` | 受管网络代理：HTTP CONNECT 策略、per-attempt 租约、网络审批流。 |
| `apps/desktop/src-tauri` | `astro-agent` | Tauri 2 桌面 shell。默认内嵌 backend（随机端口 `127.0.0.1:0`），单实例，系统托盘。Thread 事件流桥接。 |

前端 React 应用在 `apps/desktop/`，使用 CodeMirror（多语言）、`@xyflow/react`（流程图编辑器）、`react-markdown`、`lucide-react`。

### 前端 UI 架构

**主侧栏导航**（5 项）：对话 `chat`、任务 `loop`（工作流 + 定时）、文件 `files`、扩展 `skills`（Skills + MCP）、设置 `settings`。定义在 `navConfig.ts`。

**设置面板**（`settings` 页）：左侧平铺 icon 列表（11 项），右侧内容区。平铺项：通用、外观、对话、上下文与压缩、模型配置、工具、记忆、模型市场、洞察、诊断、关于。由 `SettingsTabId` 类型定义，`preferences:xxx` 前缀路由到 `PreferencesPanel` 的对应 section。

**聊天右侧面板**（`ChatRightPanel`，5 tab）：会话列表 `sessions`、任务监控 `monitor`（`TaskMonitorPanel`）、上下文 `context`（`ContextExplorer`）、预览 `preview`（`GeneratingPreviewPanel`）、Agent `agent`（`ChatAgentInfo`）。

**浮动 TODO 进度条**（`TodoProgress`）：输入区上方，从消息 activities 提取最新 `todo` 工具调用的计划状态，显示折叠进度。

## Core Architecture

### 事件管线（单一事实链）

```
AstroThread::submit(Op)
  → bounded(512) submission channel
  → Session::submission_loop — 有序 Op 分发
  → SessionTask::run_turn — 模型/工具循环
  → EventMsg — Core 单一事件格式
  → rollout policy + JSONL append（权威历史）
  → Core event queue（单 receiver）
  → Server ThreadListener（单 listener per Thread）
  → ThreadHistoryBuilder（活跃 Turn 快照）
  → per-connection bounded(128) queues
  → Tauri ThreadEventsBridge / exec / external Thread clients
```

### agent-core 模块组织

`agent` crate（`crates/agent-core`）是中央运行时，主要子模块：

- **`runtime/`** — Session 生命周期、`AstroThread`、`SessionIo`、`submission_loop`、`SessionState`、`SessionServices`、`TurnContext`、`StepContext`、`ToolRouter`、`ToolRuntime`、turn lifecycle、context maintenance、recording、system prompt
- **`tasks/`** — 可恢复任务生命周期：`SessionTask`、`ActiveTurn`、`TaskKind`、spawn/cancel/terminal 事件保证
- **`streaming/`** — 流式补全：fallback、HITL bridge、多轮 streaming、provider 抽象、tool 执行、summary
- **`exec/`** — 执行域：`AgentControlDirectory`（根级 AgentControl 进程目录）、`AgentRuntimeManager`（活跃 turn 管理）、subagents（单 turn 运行器）、dispatch（V2 6 工具分发 + 桌面控制面）、cron、background、memory review、title generation
- **`compression`** — tool 结果压缩（原文保留，压缩视图给 provider）
- **`control`** — HITL gate、中断状态机、schema 校验、smart approval（含 `SmartApprovalContext` 对话上下文）、`approval_cache`（会话级审批缓存，同命令模式不重复弹窗）、网络审批
- **`prompt/`** — 上下文组装、hook 集成、消息变换、prompt builder、sanitization

### Codex V2 Agent Thread（子 Agent 系统）

6 个模型工具：`spawn_agent`、`list_agents`、`send_message`、`followup_task`、`wait_agent`、`interrupt_agent`。桌面控制面额外提供 `read_subagent_thread` 和 `close_subagent_thread`。

与 Codex 原版差异：Codex V2 有 8 个工具（额外 `resume_agent`、`close_agent`）。Astro 精简为 6 个——`resume_agent` 的功能被 `followup_task` 吸收（可向已完成 agent 发后续任务并重新激活）；`close_agent` 从模型工具降级为桌面控制面操作（关闭 agent 是用户决策而非模型决策）。

架构：一个 `AgentControl` per 根会话，所有后代共享。`subagents.db` 拥有线程图、邮箱、状态事件和迁移元数据；`SessionStore` 保持会话/工具时间线。

状态：`PendingInit` → `Running` → `Completed { last_message }` / `Interrupted` / `Errored { message }` → `Shutdown`。

配置：从 `~/.astro/agents` 和受信任的 `<project>/.astro/agents` 加载自定义 agent 定义；设置从 `~/.astro/config.toml` 和受信任的 `<project>/.astro/config.toml` 加载。`.codex` 不作为 Astro 配置输入。

### 工具对齐（Codex → Astro）

| Codex 工具 | Astro 工具 | 说明 |
|---|---|---|
| `apply_patch` (Freeform) | `apply_patch` (Freeform) | Lark 语法 diff 补丁，支持多文件批量增删改 |
| `exec_command` | `exec_command` | Shell 命令执行，含 session 管理（`yield_time_ms`、`session_id`） |
| `write_stdin` | `write_stdin` (stub) | 向运行中 session 写入 stdin |
| `request_permissions` | `request_permissions` (stub) | 运行时请求额外权限 |
| _(无)_ | _(原 file_ops 已移除)_ | 读/搜索/列目录归入 `exec_command` |
| _(无)_ | _(原 terminal 已重命名)_ | → `exec_command` |

### Provider 架构（agent-providers）

**协议管线**（5 种 `ApiMode`）：`ChatCompletions`（OpenAI 兼容）、`Responses`（OpenAI Responses API）、`AnthropicMessages`、`Interactions`（Google Gemini）、`GeminiNative`。协议选择由 `ProviderProfile.api_mode` 默认 + `ProviderConfig.api_mode` 运行时覆盖。

**Trait 系统**：
- `OpenAICompatible` trait — 所有 OpenAI 兼容厂商的 hook 接口。通过声明式常量控制行为：
  - `THINKING_FORMAT: ThinkingFormat` — 4 种 thinking 线路格式（`None` / `ReasoningEffort` / `DeepSeek` / `MiniMaxAdaptive`）
  - `EFFORT_MAP: &[(&str, &str)]` — 推理 effort 字符串映射表
  - `SUPPORTS_RESPONSES` / `RESPONSES_STORE_FALSE` / `RESPONSES_PARALLEL_TOOLS` / `RESPONSES_REASONING_SUMMARY` — Responses API 行为标志
- `apply_thinking_compat()` — 共享 thinking 转换函数，由 `THINKING_FORMAT` + `EFFORT_MAP` 参数化
- 新增厂商只需设常量（2-4 行），不需要覆盖 `finalize_body()`

**TOML 自定义 Provider**：用户在 `~/.astro/config.toml` 中声明即可接入任何 OpenAI 兼容 API，零代码：
```toml
[custom_providers.my-corp]
base_url = "https://llm.corp.internal/v1"
env_keys = ["CORP_API_KEY"]
default_model = "corp-v3"

[[custom_providers.my-corp.models]]
id = "corp-v3"
context_window = 128000
reasoning = true
```
自定义 provider 统一走 Responses API，由 `ConfigDrivenCompletionModel` 实现。TOML 声明的模型元数据在启动时注入 `models.json` 缓存。

**模型元数据**（三层合并）：API 厂商端点发现 → OpenRouter 模型表 enrich → 已知能力补丁 (`apply_known_capability_overrides`)。合并结果持久化到 `~/.astro/cache/models.json`，前端和运行时共用。

**Provider Fallback 链**：`chat_targets: Vec<ChatTarget>` — primary + 最多 `MAX_CHAT_FALLBACKS` 个备用。首个 chunk 前失败则自动切换到下一目标。`ChatTarget.api_mode` 随链路传播到 `ProviderConfig`，确保探测和聊天走同一协议。辅助任务各有独立目标链，缺省回退到 primary。

**Profile 表**：`ProviderProfile` 静态表驱动（`PROFILES` 数组），每厂商一个条目。字段含 `supports_responses: bool`（UI 切换标志）。前端 `supports_responses_toggle()` 读此字段。

### 三总线 Hook 系统

Plugin bus 事件（Codex 对齐命名）：`PreLlmCall`、`PreToolUse`、`PermissionRequest`、`Stop`、`PreCompact`、`PostCompact`、`SessionStart`、`SessionEnd`、`UserPromptSubmit`、`SubagentStart`、`SubagentStop`、`PreApiRequest`、`PostApiRequest`、`TransformTerminalOutput`。事件名只接受 canonical 精确匹配。

### 上下文压缩

`maintain_tool_context()` 三阶段：prune（截断超大 tool 结果）→ LLM 辅模型摘要（`AuxiliaryTask::Compaction`）→ head/tail fallback。`compressed_content` 字段存 Provider 视图；`content` 字段永远保留原文。

### 可视化工作流引擎

`crates/agent-workflow` 实现 DAG 工作流：29 种节点分 6 类（Trigger 3 / AI 3 / Media 5 / FlowControl 6 / DataProcessing 7 / Action 5）。引擎模块：`dag.rs`（拓扑排序）、`executor.rs`（步骤执行）、`variables.rs`（变量插值）。前端用 `@xyflow/react` 渲染流程图编辑器。

### 目录约定

```
~/.astro/
  config.toml          # 全局统一配置（agent 设置 + MCP + custom_providers）
  agents/*.toml        # 全局自定义 agent 定义
  agents/{agent_id}/
    SOUL.md            # Agent 人格
    MEMORY.md          # 项目记忆（快照）
    USER.md            # 用户画像（快照）
    config.json        # AgentRuntimeConfig
    tools_enabled.json # tool gate 热加载
  sessions/
    state.db           # 消息、会话、FTS5（schema v23）
    artifacts.db       # 文件空间索引
    knowledge.db       # 知识内容 FTS
  cache/
    models.json        # 模型元数据缓存（API + OpenRouter + TOML 合并）
  providers.json       # 前端 Provider 配置状态
  cron/                # cron.db + jobs.json
  workflows/workflows.json
  subagents.db         # V2 Agent 线程图、邮箱、状态事件
  usage/usage.db
<project>/.astro/
  config.toml          # 受信任的项目级 agent 设置覆盖
  agents/              # 项目级 agent 定义
```

## Key Invariants

1. **角色顺序**：`session_messages` 中相邻消息不得连续出现相同 role。由 `validate_message_order()` 强制。

2. **工具深度**：`tool_rounds` 在每条用户消息开始时归零；单条用户消息内上限 `multi_turn`（默认 90）。`increment_tool_round()` 耗尽时返回 `MaxDepthError`。

3. **streaming 不变量**：每轮 assistant 回复先写入再执行工具；usage 覆盖式累加（兼容 Google 累计式 usageMetadata）。

4. **Tool spill**：tool 结果 ≥ `DEFAULT_SPILL_THRESHOLD_BYTES` 时落盘，provider history 用 stub。

5. **Skill soft-alias**：模型把 skill 名当工具调用时，自动改写为 `skills(action=load, skill_id=…)`。

6. **MCP 工具名**：`mcp__{server_id}__{tool_name}` 前缀。

7. **交互模式**：`interaction_mode` 经 ChatRequest 下传；行为说明只进 system prompt。

8. **单一事件事实链**：`EventMsg` 是 Core 唯一事件格式。SessionStore 为可重建投影；rollout JSONL 为权威历史。

9. **Agent Thread 资源守恒**：每次 spawn 失败释放路径和身份预留；每次 turn 退出释放执行槽位；completed/interrupted/errored 线程保持可寻址。

## Test Organization

集成测试在各 crate `tests/` 目录下：`agent-core`（7 个：agent_test、cron_exec_test、streaming_test、rig_agent_test、memory_snapshot_test、thread_event_lifecycle_test、tool_registry_test）、`agent-providers`（3）、`agent-subagents`（1：v2_lifecycle）、`agent-tools`（2：tools_test、hitl_tools_test）、`agent-a2ui`（3）、`agent-cron`（1）、`agent-server`（3：grpc_test、agent_thread_events、agent_thread_recovery）、`agent-session`（2）、`agent-artifacts`（1）、`agent-usage`（1）、`agent-network-proxy`（4）。前端测试在 `apps/desktop/src/`（Vitest）。
