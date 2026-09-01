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

仓库按职责分为 7 个顶层目录，共 25 个 crate：

### core/ — Agent 大脑

| 路径 | package name | 职责 |
|---|---|---|
| `crates/agent-core` | `agent` | Agent 运行时核心：`AgentLoop` 状态机、流式多轮循环、工具分发、压缩、HITL、hooks、prompt 组装。 |
| `crates/agent-providers` | `providers` | 多厂商 LLM/图像 Provider 层：`ProviderRegistry`、流式 `ChatStream`、fallback 链。支持 Google、OpenAI、Codex、DeepSeek、MiniMax、Ollama、Azure 等。 |
| `crates/agent-memory` | `memory` | `MemoryManager` — MEMORY.md/USER.md 快照、dreaming 管道、待审批记忆队列、decision log、workspace bootstrap。 |
| `crates/agent-subagents` | `subagents` | Codex V2 Agent Threads：持久化 Agent Graph/mailbox/status、自定义 agent TOML、root-scoped 控制面与恢复。 |
| `crates/agent-evolution` | `evolution` | 自进化/学习循环：改进提议、评判、信号分析、评估集、DSPy 集成。配套 Python 包 `evolution-dspy/`。 |
| `crates/agent-delegate` | `worktree` | 显式桌面多任务用的 git worktree 工具；Subagent 不会隐式创建 worktree。 |
| `crates/agent-home` | `home` | `~/.astro` 路径约定、日志、agent config YAML、tool-enable gates。无 SQLite。 |
| `crates/agent-skills` | `skills` | Skill 管理 — 安装、加载、注册表、摘要、备份。Skill frontmatter `astro_tools` 可 additive 开放 toolset。 |
| `crates/agent-sandbox` | `sandbox` | 派生进程平台沙箱、typed denial、audit 与 attempt-scoped `SandboxPolicy`；managed network 只放行已绑定代理的精确 loopback port。 |
| `crates/agent-network-proxy` | `network-proxy` | Codex 对齐的受管子进程网络边界：host allow/deny、本地地址防御、DNS rebinding 防御、decision attribution 与 attempt-scoped loopback HTTP/1 CONNECT listener；已接入前台 terminal/code_exec，plain HTTP/SOCKS 尚未实现。 |

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
| `crates/agent-protocol` | `agent-protocol` | 统一 Thread 提交与事件协议：`Op`、`EventMsg`、`TurnItem`、approval/control 与扩展事件。 |
| `crates/agent-rollout` | `agent-rollout` | append-only rollout 持久化、记录策略与 Thread 历史重建；是稳定事件恢复的事实源。 |
| `crates/agent-proto` | `proto` | Protobuf / tonic gRPC 服务契约（backend ↔ Tauri shell）。定义 `AstroService` 的 Thread submit/resume/subscribe、ChatControl、媒体、Skill、MCP、Memory、Files、Token 与 Batch RPC。 |
| `crates/agent-session` | `session` | `SessionStore`（`state.db` WAL SQLite，schema v22，FTS5）— 原生 `ResponseItem`、会话、billing、FTS 召回。 |
| `crates/agent-artifacts` | `artifacts` | 文件空间索引（`artifacts.db`）+ Knowledge Content DB（`knowledge.db`，FTS）。按来源（agent_write/user_upload/reconcile）注册文件，MIME 分类。 |
| `crates/agent-usage` | `usage` | 用量事件 DB（`usage.db`）、per-agent 统计、路由感知成本估算（官方定价快照 + OpenRouter API）、trace insights、eval JSONL 导出。 |

### automation/ — 自动化

| 路径 | package name | 职责 |
|---|---|---|
| `crates/agent-cron` | `cron` | Cron job JSON 持久化、运行记录 DB（`data/cron_v1.db`）、ticker（每 30s，`current_thread` runtime）。 |
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
      ├─ ToolCallAccumulator: 仅累积原生 tool_call_deltas（自由文本不参与工具识别）
      ├─ record_assistant_message_with_tools()
      ├─ handle_tool_call_async() × N → record_tool_result_with_id()
      └─ 无工具调用时 → 返回最终文本
```

### 网络访问

网络默认放开，不需要任何预设、审批或域名白名单：

- 子进程（`terminal`、`code_exec`、后台 job）：`build_command_sandbox_policy_with_roots`
  统一给出 `network_access = true`，macOS seatbelt 直接 `(allow network*)`，Linux 不再
  `--unshare-net`，Windows 不写 offline 标记。
- 进程内 HTTP 工具（`web_search`、`web_fetch`）：直接发请求，无一次性审批卡、无 profile
  域名裁决。唯一保留的检查是 `assert_public_http_url` 的 SSRF 防护——只允许 http/https，
  并拦截本机、私网与云 metadata 目标（重定向每一跳同样校验）。
- 远端 MCP（StreamableHttp）随之默认可连。

权限 profile 只管文件系统写入与命令沙箱模式，不再决定能否联网。

### Managed subprocess network

只有显式在 `~/.astro/config.yaml` 里开启 `network_proxy.enabled` 并让选中 custom
profile 自身 `network.enabled=true` 时，前台 `terminal action=run` 与 `code_exec`
才把流量收回受管代理，执行以下 attempt-scoped 链路：

```text
ToolOrchestrator::run
  → StartedNetworkProxy::start
  → SandboxAttempt.managed_network
  → SandboxPolicy.managed_network (exact bound port)
  → ToolExecutionGrants → ToolContext
  → terminal/code_exec env injection
  → BlockedRequest
  → SandboxErr::Denied.network_policy_decision
```

策略只取选中 leaf profile 的自有 network 字段，不继承父 profile；
`:danger-full-access`、非进程工具、进程内 HTTP、MCP/provider 和后台 terminal job
都不共享该 lease。结构化网络拒绝不进入文件系统 escalation；502/DNS/dial
错误不是 policy denial。

macOS managed sandbox 在存在 proxy lease 时自动允许 DNS，不要求用户通过
`allow_local_binding` 一并开放本地回环能力。

### agent-core 模块组织

`agent` crate 是中央运行时，8 个公开子模块，另有内部 `tasks` 生命周期模块：

- **`builder`** — 声明式 `AgentBuilder` / `BuiltAgentSpec`
- **`compression`** — tool 结果压缩（原文保留，压缩视图给 provider）
- **`control`** — HITL gate、中断状态机、schema 校验、smart approval
- **`exec`** — 执行域：cron 执行、delegate、dispatch、headless 多轮、记忆 review、mid-run summary、多 agent、编排、标题生成、tool LLM 压缩
- **`prompt`** — prompt 域：上下文组装（context/context_source/context_usage）、hook 集成、消息变换、prompt builder、sanitization
- **`runtime`** — 核心运行时：`AgentLoop`、`AgentConfig`、budget 管理、压缩状态、模型上下文、session 管理、turn budget、usage 追踪、校验
- **`streaming`** — 流式补全：fallback 处理、HITL bridge、多轮 streaming、provider 抽象、run state、summary、tool 执行
- **`timeline`** — 助手回合时间线与兼容投影

统一事件不再由 `agent-core::event_bus` 广播：Core 产生 `agent-protocol::EventMsg`，先按策略写入
`agent-rollout`，再由 Server 的每 Thread listener 投影到 gRPC/Tauri live stream；恢复使用
rollout snapshot + live boundary。

### Provider Fallback 链

`AgentLoop.chat_targets: Vec<ChatTarget>` — primary + 最多 `MAX_CHAT_FALLBACKS` 个备用。首个 chunk 前失败则自动切换到下一目标。五类 `AuxiliaryTask`（Dreaming、Compaction、SmartApproval、TitleGen 等）各有独立目标链，缺省回退到 primary。

### 三总线 Hook 系统

Plugin bus 事件（可拦截/变更）：`PreLlmCall`、`PreToolUse`、`Stop`、`TransformToolResult`、`TransformFinalLlmOutput`、`PostLlmCall`、`PostToolUse`。事件名只接受 canonical 精确匹配；Hook 返回值：`Continue`、`Block`、`Modify`、`ReplaceText`、`InjectContext`、`KeepGoing`。

### 上下文压缩

`maintain_tool_context()` 三阶段：prune（截断超大 tool 结果）→ LLM 辅模型摘要（`AuxiliaryTask::Compaction`）→ head/tail fallback。`compressed_content` 字段存 Provider 视图；`content` 字段永远保留原文。压缩后用 thrashing guard 防抖（同一轮连续压缩不生效）。

### Cron 的 non-Send 约束

`AgentLoop` / `SessionStore` 含 rusqlite `RefCell`，非 Send。Cron 必须在 `current_thread` runtime 运行，通过 `spawn_blocking` 封装后对外暴露 Send future。

### Agent Threads

`crates/agent-subagents` 是唯一 Subagent 模型。模型只有六个工具：`spawn_agent`、`list_agents`、`send_message`、`followup_task`、`wait_agent`、`interrupt_agent`。`send_message` 只入队，`followup_task` 入队并触发/恢复 turn，`wait_agent` 等待任意 mailbox/final/steer 活动。read 真实 Session 时间线和递归 close 只是 Desktop 控制面操作，不是模型工具。状态固定为 `PendingInit` / `Running` / `Interrupted` / `Completed` / `Errored` / `Shutdown`。Agent Graph/mailbox/status 写入 `~/.astro/data/subagents-v2.db`，真实对话写入 `~/.astro/data/state.db`；旧 V1 表仅在迁移时转为只读历史归档。凭证只在内存中传递，权限继承父任务且自定义 agent 仅可收窄，不隐式创建 git worktree。自定义 agent 和设置只从 `~/.astro/agents/*.toml`、`<project>/.astro/agents/*.toml`、`~/.astro/config.toml` 和可信项目的 `<project>/.astro/config.toml` 加载，project 定义优先；`.codex` 不作为 Astro 配置输入。

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
  data/
    state.db           # ResponseItem、会话、FTS5（schema v22）
    artifacts.db       # 文件空间索引
    knowledge.db       # 知识内容 FTS
    subagents-v2.db    # Agent Graph、mailbox、状态事件与恢复元数据
    usage.db           # 用量和成本事件
    cron_v1.db         # Cron 运行记录
  sessions/rollouts/   # append-only 事件事实源
  cron/                # jobs.json
  workflows/workflows.json
```

## Key Invariants

1. **原生历史**：Agent、rollout、SQLite 和 Desktop history RPC 都使用 `ResponseItem`。其中相邻的 user/assistant message item 不得重复角色，由 `validate_message_order()` 强制。

2. **工具深度**：`tool_rounds` 在每条用户消息开始时归零（`begin_user_turn`）；单条用户消息内上限 `multi_turn`（默认 90，对齐 Hermes）。`increment_tool_round()` 在耗尽时返回 `MaxDepthError`。

3. **streaming 不变量**：模型产生的 assistant/call items 必须先写入 `response_items`，再执行工具并写入 matching output item；`code_exec` 独占轮可退还预算；usage 覆盖式累加（兼容 Google 累计式 usageMetadata）。

4. **Tool spill**：tool 结果 ≥ `DEFAULT_SPILL_THRESHOLD_BYTES` 时落盘，item metadata 的 `astro_compressed_output` 存 stub view，provider history 用 stub 而非原文。

5. **Skill soft-alias**：模型把 skill 名当工具调用时，若工具注册表中不存在但 skill 已安装且 enabled，自动改写为 `skills(action=load, skill_id=…)`。

6. **MCP 工具名**：`mcp__{server_id}__{tool_name}` 前缀，`is_mcp_tool_name()` 检测。

7. **交互模式**：`interaction_mode` 经 Agent turn 下传；行为说明只进 Responses `instructions`，用户 item 不得拼接 `[Mode: …]`。`start_chat` 仅接受 `StartChatRequest` 包装，无扁平字段兼容。

8. **网络默认放开**：沙箱策略一律 `network_access = true`，进程内 HTTP 工具只保留 SSRF 防护。开启 managed proxy 后，proxy listener 只归属单个 tool attempt，沙箱只放行其精确端口；terminal 后台模式在 spawn 前拒绝，code_exec 先 scrub secrets 再注入 proxy env，结构化网络拒绝不得触发文件系统提权。

## Data Flow: cron.rs → headless.rs

定时任务入口：`execute_job` → `execute_job_with_roots_local` → `run_agent_job`。
`run_agent_job` 调用 `AgentLoop::run_turn` 完成初始化（记录用户消息、构建 system prompt），
然后将控制权交给 `exec::headless::run_headless_multi_turn`，后者与 `run_multi_turn_stream_inner` 共享核心逻辑（tool_call_deltas 累积、IterationBudget、maintain_tool_context、provider_history、inject_context）。

## Test Organization

集成测试在各 crate `tests/` 目录下，覆盖核心路径：`crates/agent-core`、`crates/agent-subagents`、`crates/agent-providers`、`crates/agent-tools`、`crates/agent-a2ui`、`crates/agent-cron`、`crates/agent-server`、`crates/agent-session`、`crates/agent-artifacts`、`crates/agent-usage`。测试环境工具：`crates/agent-home/src/test_env.rs`。前端测试在 `apps/desktop/src/a2ui/`。
