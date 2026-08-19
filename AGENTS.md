# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

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
cargo test -p agent finalize_tool_call_result_keeps_raw_delegate
cargo test -p agent finalize_tool_call_result -- --nocapture

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

仓库按职责分为 7 个顶层目录，共 23 个 crate：

### core/ — Agent 大脑

| 路径 | package name | 职责 |
|---|---|---|
| `crates/agent-core` | `agent` | Agent 运行时核心：`AgentLoop` 状态机、流式多轮循环、工具分发、压缩、HITL、hooks、prompt 组装。 |
| `crates/agent-providers` | `providers` | 多厂商 LLM/图像 Provider 层：`ProviderRegistry`、流式 `ChatStream`、fallback 链。支持 Google、OpenAI、Codex、DeepSeek、MiniMax、Ollama、Azure 等。 |
| `crates/agent-memory` | `memory` | `MemoryManager` — MEMORY.md/USER.md 快照、dreaming 管道、待审批记忆队列、decision log、workspace bootstrap。 |
| `crates/agent-subagents` | `subagents` | Codex 风格 Agent Threads：持久化、自定义 agent TOML、spawn/list/read/send/wait/interrupt/close 生命周期。 |
| `crates/agent-evolution` | `evolution` | 自进化/学习循环：改进提议、评判、信号分析、评估集、DSPy 集成。配套 Python 包 `evolution-dspy/`。 |
| `crates/agent-delegate` | `worktree` | 显式桌面多任务用的 git worktree 工具；Subagent 不会隐式创建 worktree。 |
| `crates/agent-home` | `home` | `~/.astro` 路径约定、日志、agent config YAML、tool-enable gates。无 SQLite。 |
| `crates/agent-skills` | `skills` | Skill 管理 — 安装、加载、注册表、摘要、备份。Skill frontmatter `astro_tools` 可 additive 开放 toolset。 |
| `crates/agent-sandbox` | `sandbox` | 派生进程平台沙箱、typed denial、audit 与 attempt-scoped `SandboxPolicy`。 |
| `crates/agent-network-proxy` | `network-proxy` | Codex 对齐的受管子进程网络边界：host allow/deny、本地地址防御、decision attribution 与 loopback HTTP/1 CONNECT listener；plain HTTP/SOCKS 与子进程注入待接入。 |

### actions/ — 工具实现

| 路径 | package name | 职责 |
|---|---|---|
| `crates/agent-tools` | `tools` | 全部内置工具实现（`register_all`）、注册表/分发、审批逻辑、HITL、schema sanitization。工具域：terminal、file_ops、browser、code_exec、memory、skills、subagents、media（image_gen/tts/video/music）等。 |
| `crates/agent-a2ui` | `a2ui` | AG-UI 声明式生成式 UI 表面：22 种组件（Text、Card、Button、Image、Audio、Video、Metric、ClarifyWizard 等）、模板、校验。Catalog ID: `astro://a2ui/catalog/v2`。 |

### channels/ — 对外通道

| 路径 | package name | 职责 |
|---|---|---|
| `crates/agent-server` | `server` | 独立 gRPC 服务端（tonic）。`run_embedded()` 供 Tauri in-process 使用；Cron ticker 跑在 side thread。 |

### storage/ — 持久化与共享类型

| 路径 | package name | 职责 |
|---|---|---|
| `crates/agent-types` | `types` | 跨 crate 共享类型：`Message`、`Role`、`ToolCall`、`MediaAsset`、`ChatTarget`、`ModelSpec`、SQLite helpers、tool-spill。无业务逻辑。 |
| `crates/agent-proto` | `proto` | Protobuf / tonic gRPC 服务契约（backend ↔ Tauri shell）。定义 `AstroService` 14 个 RPC（Chat、ChatControl、GenerateImage、ListSkills、ExecuteSkill、ListMcpServers、QueryMemory、SubscribeSessionEvents 等）。 |
| `crates/agent-session` | `session` | `SessionStore`（`state.db` WAL SQLite，schema v17，FTS5）— 消息、会话、billing、FTS 召回。 |
| `crates/agent-artifacts` | `artifacts` | 文件空间索引（`artifacts.db`）+ Knowledge Content DB（`knowledge.db`，FTS）。按来源（agent_write/user_upload/reconcile）注册文件，MIME 分类。 |
| `crates/agent-usage` | `usage` | 用量事件 DB（`usage.db`）、per-agent 统计、路由感知成本估算（官方定价快照 + OpenRouter API）、trace insights、eval JSONL 导出。 |

### automation/ — 自动化

| 路径 | package name | 职责 |
|---|---|---|
| `crates/agent-cron` | `cron` | Cron job JSON 持久化、运行记录 DB（`cron.db`）、ticker（每 30s，`current_thread` runtime）。 |
| `crates/agent-workflow` | `workflow` | 可视化工作流引擎：29 种节点跨 6 类（Trigger、AI、Media、FlowControl、DataProcessing、Action），DAG 执行引擎、变量解析、运行 DB。持久化 `~/.astro/workflows/workflows.json`。 |

### extensions/ — 可插拔扩展

| 路径 | package name | 职责 |
|---|---|---|
| `crates/agent-mcp` | `mcp` | MCP 客户端 — per-agent 进程级连接池（`McpHub`），工具发现与调用。工具名约定：`mcp__{server}__{tool}`。 |
| `crates/agent-hooks` | `hooks` | 三总线 hook 系统：Plugin（进程内同步 `PluginHookBus`）、Gateway（文件扫描外部 manifest）、Shell（config-map 异步 shell 命令）。 |

### ui/ — 前端

| 路径 | package name | 职责 |
|---|---|---|
| `apps/desktop/src-tauri` | `astro-agent` | Tauri 2 桌面 shell。默认内嵌 backend（随机端口 `127.0.0.1:0`），单实例，系统托盘。32 个 Tauri command 模块。 |

前端 React 应用在 `apps/desktop/`，使用 CodeMirror（多语言）、`@xyflow/react`（流程图编辑器，用于 workflow）、`react-markdown`、`lucide-react`。

## Core Architecture

### Agent 请求生命周期

```
用户输入
  → AgentLoop::run_turn()
      ├─ begin_user_turn()      // 重置 tool_rounds、compression guard
      ├─ reload_tools_and_mcp() // 热加载 tool gates + MCP（每轮）
      ├─ SessionStore::append_message()
      ├─ build_conversation_context() // FTS 召回（turn >= recent_turns=10 时触发）
      ├─ build_system_prompt()  // soul + MEMORY + USER + daily + recalled + skills + interactionMode guidance + TOOL_GUIDANCE
      └─ → TurnResult::Continue { system_prompt }

  → run_multi_turn_stream() (streaming 路径)
  → run_headless_multi_turn() (cron/无流式路径)
      ├─ loop: maintain_tool_context → reload → LLM stream → accumulate
      ├─ ToolCallAccumulator: native tool_call_deltas + XML <tool_call> 统一解析
      ├─ record_assistant_message_with_tools()
      ├─ handle_tool_call_async() × N → record_tool_result_with_id()
      └─ 无工具调用时 → 返回最终文本
```

### agent-core 模块组织

`agent` crate 是中央运行时，7 个子模块：

- **`builder`** — 声明式 `AgentBuilder` / `BuiltAgentSpec`
- **`compression`** — tool 结果压缩（原文保留，压缩视图给 provider）
- **`control`** — HITL gate、中断状态机、schema 校验、smart approval
- **`event_bus`** — agent 事件广播（UI 流式订阅）
- **`exec`** — 执行域：cron 执行、delegate、dispatch、headless 多轮、记忆 review、mid-run summary、多 agent、编排、标题生成、tool LLM 压缩
- **`prompt`** — prompt 域：上下文组装（context/context_source/context_usage）、hook 集成、消息变换、prompt builder、sanitization
- **`runtime`** — 核心运行时：`AgentLoop`、`AgentConfig`、budget 管理、压缩状态、模型上下文、session 管理、turn budget、usage 追踪、校验
- **`streaming`** — 流式补全：fallback 处理、HITL bridge、多轮 streaming、provider 抽象、run state、summary、tool 执行

### Provider Fallback 链

`AgentLoop.chat_targets: Vec<ChatTarget>` — primary + 最多 `MAX_CHAT_FALLBACKS` 个备用。首个 chunk 前失败则自动切换到下一目标。五类 `AuxiliaryTask`（Dreaming、Compaction、SmartApproval、TitleGen 等）各有独立目标链，缺省回退到 primary。

### 三总线 Hook 系统

Plugin bus 事件（可拦截/变更）：`pre_llm_call`、`pre_tool_call`、`pre_verify`、`transform_tool_result`、`transform_llm_output`、`post_llm_call`、`post_tool_call`。Hook 返回值：`Continue`、`Block`、`Modify`、`ReplaceText`、`InjectContext`、`KeepGoing`。

### 上下文压缩

`maintain_tool_context()` 三阶段：prune（截断超大 tool 结果）→ LLM 辅模型摘要（`AuxiliaryTask::Compaction`）→ head/tail fallback。`compressed_content` 字段存 Provider 视图；`content` 字段永远保留原文。压缩后用 thrashing guard 防抖（同一轮连续压缩不生效）。

### Cron 的 non-Send 约束

`AgentLoop` / `SessionStore` 含 rusqlite `RefCell`，非 Send。Cron 必须在 `current_thread` runtime 运行，通过 `spawn_blocking` 封装后对外暴露 Send future。

### Agent Threads

`crates/agent-subagents` 是唯一 Subagent 模型。父 Agent 用 `spawn_agent` 启动独立线程，并可通过 `list_agents` / `read_agent` / `followup_task` / `wait_agent` / `interrupt_agent` / `close_agent` 管理（旧名 `send_message_to_agent` / `wait_agents` 兼容）。线程持久化到 `~/.astro/subagents.db`，凭证只在内存中传递，权限继承父任务且自定义 agent 仅可收窄，不隐式创建 git worktree。自定义 agent 规范路径为 `~/.codex/agents/*.toml` 和 `<project>/.codex/agents/*.toml`，`.astro/agents` 作为兼容层，project `.codex` 定义优先。

### 可视化工作流引擎

`crates/agent-workflow` 实现 DAG 工作流：29 种节点分 6 类（Trigger 3 / AI 3 / Media 5 / FlowControl 6 / DataProcessing 7 / Action 5）。引擎模块：`dag.rs`（拓扑排序）、`executor.rs`（步骤执行）、`variables.rs`（变量插值）。前端用 `@xyflow/react` 渲染流程图编辑器。

### 目录约定

```
~/.astro/
  agents/{agent_id}/
    SOUL.md            # Agent 人格
    MEMORY.md          # 项目记忆（快照）
    USER.md            # 用户画像（快照）
    config.json        # AgentRuntimeConfig
    tools_enabled.json # tool gate 热加载
  sessions/
    state.db           # 消息、会话、FTS5（schema v17）
    artifacts.db       # 文件空间索引
    knowledge.db       # 知识内容 FTS
  cron/                # cron.db + jobs.json
  workflows/workflows.json
  subagents.db          # Agent Thread 状态与消息
  usage/usage.db
```

## Key Invariants

1. **角色顺序**：`session_messages` 中相邻消息不得连续出现相同 role（user/user 或 assistant/assistant）。由 `validate_message_order()` 强制。

2. **工具深度**：`tool_rounds` 在每条用户消息开始时归零（`begin_user_turn`）；单条用户消息内上限 `multi_turn`（默认 90，对齐 Hermes）。`increment_tool_round()` 在耗尽时返回 `MaxDepthError`。

3. **streaming 不变量**：每轮 assistant 回复必须先写入 `session_messages`（含 `tool_calls` 字段）再执行工具；`code_exec` 独占轮可退还预算；usage 覆盖式累加（兼容 Google 累计式 usageMetadata）。

4. **Tool spill**：tool 结果 ≥ `DEFAULT_SPILL_THRESHOLD_BYTES` 时落盘，`compressed_content` 存 stub view，provider history 用 stub 而非原文。

5. **Skill soft-alias**：模型把 skill 名当工具调用时，若工具注册表中不存在但 skill 已安装且 enabled，自动改写为 `skills(action=load, skill_id=…)`。

6. **MCP 工具名**：`mcp__{server_id}__{tool_name}` 前缀，`is_mcp_tool_name()` 检测。

7. **交互模式**：`interaction_mode` 经 ChatRequest 下传；行为说明只进 system（`system_guidance`），用户消息不得拼接 `[Mode: …]`。`start_chat` 仅接受 `StartChatRequest` 包装，无扁平字段兼容。schema v17 起剥离历史 Mode 后缀。

## Data Flow: cron.rs → headless.rs

定时任务入口：`execute_job` → `execute_job_with_roots_local` → `run_agent_job`。
`run_agent_job` 调用 `AgentLoop::run_turn` 完成初始化（记录用户消息、构建 system prompt），
然后将控制权交给 `exec::headless::run_headless_multi_turn`，后者与 `run_multi_turn_stream_inner` 共享核心逻辑（tool_call_deltas 累积、IterationBudget、maintain_tool_context、provider_history、inject_context）。

## Test Organization

集成测试在各 crate `tests/` 目录下，覆盖核心路径：`crates/agent-core`、`crates/agent-subagents`、`crates/agent-providers`、`crates/agent-tools`、`crates/agent-a2ui`、`crates/agent-cron`、`crates/agent-server`、`crates/agent-session`、`crates/agent-artifacts`、`crates/agent-usage`。测试环境工具：`crates/agent-home/src/test_env.rs`。前端测试在 `apps/desktop/src/a2ui/`。
