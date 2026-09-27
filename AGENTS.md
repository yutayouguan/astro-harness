# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## 工作规则与项目配置

项目工作规则统一放在项目根目录的 `AGENTS.md`，不放入 `.astro`。运行时叠加全局工作区的 `AGENTS.md` 与本文件；冲突时项目规则优先，其余全局工作原则继续适用，不覆盖全局人格、身份、用户偏好或工具环境。

项目 `.astro/` 仅存放项目特有配置（例如显式配置的 `config.toml`、自定义 Agent 定义）。没有项目特有配置时无需创建该目录；读取工作规则或打开项目不得为此创建 `.astro/`。既有项目配置不得因规则路径调整而删除。

Provider 注册信息、工具/Skill 开关和 Agent 默认设置统一在全局 `config.toml` 的 `desktop` 段读写；数据库、rollout、缓存和使用统计仍是运行数据。旧 JSON 必须通过显式迁移导入，运行时不双写或回退；参见 `docs/global-settings.md`。写 TOML 时使用 `home::settings` 的同一文件锁和分段更新，不覆盖其他配置段。

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

仓库按职责分为 7 个顶层目录，共 28 个 crate：

### core/ — Agent 大脑

| 路径 | package name | 职责 |
|---|---|---|
| `crates/agent-core` | `agent` | Agent 运行时核心：`AgentLoop` 状态机、流式多轮循环、工具分发、压缩、HITL、hooks、prompt 组装。 |
| `crates/agent-providers` | `providers` | 多厂商 LLM/图像 Provider 层：`ProviderRegistry`、流式 `ChatStream`、fallback 链。支持 Google、OpenAI、Codex、DeepSeek、MiniMax、Ollama、Azure 等。 |
| `crates/agent-memory` | `memory` | `MemoryManager` — MEMORY.md/USER.md 快照、dreaming 管道、待审批记忆队列、decision log、workspace bootstrap。 |
| `crates/agent-subagents` | `subagents` | Codex V2 Agent Threads：持久化 Agent Graph/mailbox/status、自定义 agent TOML、root-scoped 控制面与恢复。 |
| `crates/agent-evolution` | `evolution` | 自进化/学习循环：改进提议、评判、信号分析、评估集、DSPy 集成。配套 Python 包 `evolution-dspy/`。 |
| `crates/agent-delegate` | `worktree` | 显式桌面多任务用的 git worktree 工具；Subagent 不会隐式创建 worktree。 |
| `crates/agent-home` | `home` | `~/.astro` 路径约定、日志、全局 TOML（含桌面与 Agent 默认设置）、tool-enable gates。无 SQLite。 |
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
| `crates/agent-types` | `types` | 跨 crate 共享类型：`ModelTarget`、`model_tool::ToolCall`、`MediaAsset`、`ModelSpec`、SQLite helpers、tool-spill。Agent history 类型归 `agent-protocol`。 |
| `crates/agent-protocol` | `agent-protocol` | 统一 Thread 提交与事件协议：`Op`、`EventMsg`、`TurnItem`、approval/control 与扩展事件。 |
| `crates/agent-rollout` | `agent-rollout` | append-only rollout 持久化、记录策略与 Thread 历史重建；是稳定事件恢复的事实源。 |
| `crates/agent-proto` | `proto` | Protobuf / tonic gRPC 服务契约（backend ↔ Tauri shell）。定义 `AstroService` 的 Thread submit/resume/subscribe、ChatControl、媒体、Skill、MCP、Memory、Files、Token 与 Batch RPC。 |
| `crates/agent-session` | `session` | `SessionStore`（`state.db` WAL SQLite，schema v24，FTS5）— 原生 `ResponseItem`、会话、billing、FTS 召回、线程检查点与线程附件。 |
| `crates/agent-artifacts` | `artifacts` | 文件空间索引（`artifacts.db`）+ Knowledge Content DB（`knowledge.db`，FTS）。按来源（agent_write/user_upload/reconcile）注册文件，MIME 分类。 |
| `crates/agent-usage` | `usage` | 用量事件 DB（`usage.db`）、per-agent 统计、路由感知成本估算（官方定价快照 + OpenRouter API）、trace insights、eval JSONL 导出。 |

### automation/ — 自动化

| 路径 | package name | 职责 |
|---|---|---|
| `crates/agent-cron` | `cron` | Cron job JSON 持久化、运行记录 DB（`automation/cron/cron_v1.db`）、ticker（每 30s，`current_thread` runtime）。 |
| `crates/agent-workflow` | `workflow` | 可视化工作流引擎：29 种节点跨 6 类（Trigger、AI、Media、FlowControl、DataProcessing、Action），DAG 执行引擎、变量解析、运行 DB。持久化 `~/.astro/automation/workflows/workflows.json`。 |

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

  → install_multi_turn_task() / run_multi_turn_events() (统一事件驱动路径)
      └─ run_background_multi_turn() (cron/子任务适配器)
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

只有显式在 `~/.astro/config.toml` 里开启 `network_proxy.enabled` 并让选中 custom
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
- **`exec`** — 执行域：cron 执行、background adapter、delegate、dispatch、记忆 review、mid-run summary、多 agent、编排、标题生成、tool LLM 压缩
- **`prompt`** — prompt 域：上下文组装（context/context_source/context_usage）、hook 集成、消息变换、prompt builder、sanitization
- **`runtime`** — 核心运行时：`AgentLoop`、`AgentConfig`、budget 管理、压缩状态、模型上下文、session 管理、turn budget、usage 追踪、校验
- **`streaming`** — 流式补全：fallback 处理、HITL bridge、多轮 streaming、provider 抽象、run state、summary、tool 执行
- **`timeline`** — 助手回合时间线与兼容投影

统一事件不再由 `agent-core::event_bus` 广播：Core 产生 `agent-protocol::EventMsg`，先按策略写入
`agent-rollout`，再由 Server 的每 Thread listener 投影到 gRPC/Tauri live stream；恢复使用
rollout snapshot + live boundary。

### Provider Fallback 链

`AgentLoop.model_targets: Vec<ModelTarget>` — primary + 最多 `MAX_MODEL_FALLBACKS` 个备用。首个 chunk 前失败则自动切换到下一目标。五类 `AuxiliaryTask`（Dreaming、Compaction、SmartApproval、TitleGen 等）各有独立目标链，缺省回退到 primary。

### 三总线 Hook 系统

Plugin bus 事件（可拦截/变更）：`PreLlmCall`、`PreToolUse`、`Stop`、`TransformToolResult`、`TransformFinalLlmOutput`、`PostLlmCall`、`PostToolUse`。事件名只接受 canonical 精确匹配；Hook 返回值：`Continue`、`Block`、`Modify`、`ReplaceText`、`InjectContext`、`KeepGoing`。

### 上下文压缩

`maintain_tool_context()` 三阶段：prune（截断超大 tool 结果）→ LLM 辅模型摘要（`AuxiliaryTask::Compaction`）→ head/tail fallback。`compressed_content` 字段存 Provider 视图；`content` 字段永远保留原文。压缩后用 thrashing guard 防抖（同一轮连续压缩不生效）。

`notes` 原子维护当前线程检查点（revision CAS，最多 8000 字符），不混入长期记忆；`history` 按 canonical rollout 物理行引用提供当前线程只读 list/search/read。`get_context_remaining` 使用最近采样占用，`new_context_window` 只排队绑定 turn 的请求，由完整工具批次落盘后的维护边界执行并报告结果。检查点作为独立 user 上下文注入，历史回退后标为过时。schema v22 → v23 增量添加检查点，v23 → v24 增量添加线程附件，均不重建原有会话数据。

### Cron 的 non-Send 约束

`AgentLoop` / `SessionStore` 含 rusqlite `RefCell`，非 Send。Cron 必须在 `current_thread` runtime 运行，通过 `spawn_blocking` 封装后对外暴露 Send future。

### Agent Threads

`crates/agent-subagents` 是唯一 Subagent 模型。模型只有六个工具：`spawn_agent`、`list_agents`、`send_message`、`followup_task`、`wait_agent`、`interrupt_agent`。`send_message` 只入队，`followup_task` 入队并触发/恢复 turn，`wait_agent` 等待任意 mailbox/final/steer 活动。read 真实 Session 时间线和递归 close 只是 Desktop 控制面操作，不是模型工具。状态固定为 `PendingInit` / `Running` / `Interrupted` / `Completed` / `Errored` / `Shutdown`。Agent Graph/mailbox/status 写入 `~/.astro/sessions/subagents/subagents-v2.db`，真实对话写入 `~/.astro/sessions/state.db`；旧 V1 表仅在迁移时转为只读历史归档。凭证只在内存中传递，权限继承父任务且自定义 agent 仅可收窄，不隐式创建 git worktree。root-scoped `AgentControl` 共享最新 service tier，子孙 Agent 的新 turn 在 OpenAI/Codex backend 上继承该 tier，不支持的 backend 不透传。自定义 agent 和设置只从 `~/.astro/agents/*.toml`、`<project>/.astro/agents/*.toml`、`~/.astro/config.toml` 和可信项目的 `<project>/.astro/config.toml` 加载，project 定义优先；`.codex` 不作为 Astro 配置输入。

### 可视化工作流引擎

`crates/agent-workflow` 实现 DAG 工作流：29 种节点分 6 类（Trigger 3 / AI 3 / Media 5 / FlowControl 6 / DataProcessing 7 / Action 5）。引擎模块：`dag.rs`（拓扑排序）、`executor.rs`（步骤执行）、`variables.rs`（变量插值）。前端用 `@xyflow/react` 渲染流程图编辑器。

### 目录约定

```
~/.astro/
  config.toml          # 唯一全局配置入口（原 YAML 已退役）
  .env                 # 凭证环境入口
  agents/              # *.toml 自定义 Agent；active.json 当前专家标识
  models/              # cache/ 模型元数据、定价；Provider 设置在 config.toml
  tools/               # 工具领域运行数据；tool gate 在 config.toml
  skills/              # 技能包、origins.json、lock.json、backups/；开关在 config.toml
  sessions/
    state.db           # ResponseItem、会话、FTS5、线程检查点与附件（schema v24）
    rollouts/          # append-only 事件事实源
    tool_spills/       # 大工具输出
    subagents/subagents-v2.db # Agent Graph、mailbox、状态事件
  artifacts/           # artifacts.db、knowledge.db、uploads/
  usage/               # usage.db 与 agents/{id}/stats.json
  automation/
    cron/              # jobs.json、cron_v1.db、output/
    workflows/         # workflows.json、workflow.db、schedule_state.json、backups/
  memory/              # dreaming.json、pending/；不含安全审计
  evolution/           # 学习、决策、进化记录与 dspy/.venv
  security/            # audit/、locks/
  browser/             # 浏览器配置及登录 profile，不是可随意删除的缓存
  ui/                  # onboarding.json、图标、壁纸、主题、桌宠
  logs/                # 运行日志
  workspace/           # SOUL.md、USER.md、MEMORY.md、AGENTS.md、日记与生成物
  backups/             # 离线迁移备份与清单
```

路径统一使用 `home::layout`（由 `home` 顶层导出）；禁止业务模块重拼领域路径。
新运行时不读写旧目录，也不在启动时搬迁数据。旧安装必须离线迁移；发现旧布局或
未完成迁移时拒绝初始化，避免生成空的平行数据库。详细布局与迁移边界见
`docs/home-layout.md`。项目根 `AGENTS.md` 和项目 `.astro/config.toml` 的边界不变。

可编辑 JSON 设置的迁移必须在目录迁移完成后执行，使用 `astro-migrate-config` 的预览与 `--apply`；旧 JSON 保留备份但不作为运行时回退。详见 `docs/global-settings.md`。

## Key Invariants

1. **原生历史**：Agent、rollout、SQLite 和 Desktop history RPC 都使用 `ResponseItem`。其中相邻的 user/assistant message item 不得重复角色，由 `validate_message_order()` 强制。
通用 assistant/tool/hook item 先写 canonical rollout，再写 SQLite 查询投影；mailbox/steer user input 使用带 durable marker 的两阶段准入。

2. **工具深度**：`tool_rounds` 在每条用户消息开始时归零（`begin_user_turn`）；单条用户消息内上限 `multi_turn`（默认 90，对齐 Hermes）。`increment_tool_round()` 在耗尽时返回 `MaxDepthError`。

3. **streaming 不变量**：模型产生的 assistant/call items 必须先写入 `response_items`，再执行工具并写入 matching output item；`code_exec` 独占轮可退还预算；usage 覆盖式累加（兼容 Google 累计式 usageMetadata）。

4. **Tool spill**：tool 结果 ≥ `DEFAULT_SPILL_THRESHOLD_BYTES` 时落盘，item metadata 的 `astro_compressed_output` 存 stub view，provider history 用 stub 而非原文。

5. **Skill soft-alias**：模型把 skill 名当工具调用时，若工具注册表中不存在但 skill 已安装且 enabled，自动改写为 `skills(action=load, skill_id=…)`。

6. **MCP 工具名**：`mcp__{server_id}__{tool_name}` 前缀，`is_mcp_tool_name()` 检测。

7. **交互模式**：`interaction_mode` 经 Agent turn 下传；行为说明只进 Responses `instructions`，用户 item 不得拼接 `[Mode: …]`。`start_chat` 仅接受 `StartChatRequest` 包装，无扁平字段兼容。

10. **模型发起的权限升级**：`request_permissions` 只能经 confirm preflight 的 HITL 批准生效，且必须同时满足：路径净化（绝对路径，两侧规范化后拒绝 Astro 自身目录与 `.ssh`/`.aws`/`.gnupg`/`Library/Keychains`；配置文件声明的可写根在加载时同样过滤并记 `dropped_write_root` 诊断）、去重（先与当前有效范围——工作区根 + 会话授权 + 永久可写目录——比对：已覆盖的根不弹卡也不落审计，完全访问模式直接返回无需请求，只请求真正越界的根）、**会话级**（`Session::permission_grants`，仅内存、随会话结束失效、可在权限菜单撤销）、审计齐全（permission.requested/granted/denied/revoked）。升为**永久**只走用户确认的设置流（权限菜单「写入永久」/权限设置页，写 `permissions.extra_writable_roots`，对所有 profile 生效且可逐条移除）；模型没有写配置的通道。沙箱构造跳过已不存在的可写根（丢根只会更窄，不让整轮调用失败）。答复走 confirm 的 HITL resume——不要接入 `Op::RequestPermissionsResponse`（它没有客户端卡片，`EventMsg::RequestPermissions` 只用于 rollout/观察者留痕）。

9. **工具可用性回调**：`ToolEntry::check_fn` 在每次 Step 构建模型可见 schema 时被**同步**求值，必须是 O(1) 探测（列一层目录、读环境变量、查内存状态）；禁止递归遍历、全盘扫描、子进程或网络调用。历史事故：浏览器探测递归扫 `~/Library/Caches/ms-playwright`，16 个工具每轮约 1.8s 同步阻塞异步 worker，任务取消/替换因此无法在预算内生效。

8. **网络默认放开**：沙箱策略一律 `network_access = true`，进程内 HTTP 工具只保留 SSRF 防护。开启 managed proxy 后，proxy listener 只归属单个 tool attempt，沙箱只放行其精确端口；terminal 后台模式在 spawn 前拒绝，code_exec 先 scrub secrets 再注入 proxy env，结构化网络拒绝不得触发文件系统提权。

## Data Flow: cron.rs → background.rs

定时任务入口：`execute_job` → `execute_job_with_roots_local` → `run_agent_job`。
`run_agent_job` 构造 Session 与模型目标，然后交给
`exec::background::run_background_multi_turn`。Background adapter 通过
`streaming::install_multi_turn_task` 进入与前台相同的 `SessionTask` / 事件执行链，
不再维护独立的 headless 循环。

## Test Organization

集成测试在各 crate `tests/` 目录下，覆盖核心路径：`crates/agent-core`、`crates/agent-subagents`、`crates/agent-providers`、`crates/agent-tools`、`crates/agent-a2ui`、`crates/agent-cron`、`crates/agent-server`、`crates/agent-session`、`crates/agent-artifacts`、`crates/agent-usage`。测试环境工具：`crates/agent-home/src/test_env.rs`。前端测试在 `apps/desktop/src/a2ui/`。

改动进程级环境变量（`ASTRO_MEMORY_DIR` 等）的测试必须使用 `home::test_env::AstroMemoryDirGuard`：它持有跨 crate 共享的锁并在 Drop 时还原。禁止裸 `std::env::set_var` + 手动清理，也不要再新增 crate 私有的 env 锁——同一个测试二进制里存在多把锁等于没有锁（曾导致 mcp/skills/usage 三个 crate 稳定失败或间歇失败）。

基线命令：`cargo test --workspace --no-fail-fast`（期望 0 失败）、`cd apps/desktop && node scripts/run-tests.mjs`、`npx tsc --noEmit`、`npx playwright test`。可用 `node tools/verify-tests.mjs` 一次跑完（默认 quick：workspace 测试 + 前端单测 + 类型检查；`--full` 追加 Playwright 全量）。
