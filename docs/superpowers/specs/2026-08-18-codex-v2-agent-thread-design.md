# Codex V2 Agent Thread 唯一契约重构设计

**日期：** 2026-08-18

**状态：** 已实施；运行契约以 `docs/subagents.md` 和 V2 代码为准

**范围：** `agent-subagents` 运行时、模型工具契约、持久化、桌面控制面、前端 Agent Tree UI、配置加载与历史数据迁移

## 1. 背景

Astro 已有 `crates/agent-subagents`、Subagent 活动条和桌面面板，但当前实现仍是“直接子线程 + 轮询 + 兼容别名”模型，与 Codex V2 Agent Thread 的树形控制、mailbox 通信和事件驱动生命周期存在结构差异。

本设计不再叠加新兼容层，而是硬切到 Codex V2 模型：

- 模型只能看到 6 个 V2 Agent Thread 工具；
- `read` 和 `close` 仅属于桌面控制面；
- 删除旧工具名、旧参数别名和 Astro agent 配置兼容路径；
- 保留旧线程数据的一次性历史迁移，但不保留旧运行时语义。

## 2. 目标与非目标

### 2.1 目标

1. 以 Codex V2 为唯一模型工具契约，确保工具名、参数和语义一致。
2. 用共享 `AgentControl` 管理整棵 Agent Tree，支持多层 spawn、列表、发送、追加任务、等待和中断。
3. 用持久化 mailbox 表达 agent 之间的通信，严格区分“排队消息”和“触发新 turn”。
4. 状态只由真实 runner 事件推导，禁止 API 乐观写入终态。
5. 用现有 SessionStore 作为完整对话和工具时间线的唯一真实来源。
6. 前端从固定轮询改为“初始快照 + 会话事件增量更新”，在主会话内呈现实时 Agent Tree 活动。
7. 在崩溃、重启、初始化失败和重复请求下保持路径、配额、mailbox 和状态一致。

### 2.2 非目标

- Subagent 不隐式创建 git worktree；显式多任务隔离仍由 `agent-delegate` 负责。
- 不将 `~/.astro` 运行数据主目录更名为 `~/.codex`。
- 不将旧线程转换为可继续执行的 V2 线程。
- 不把桌面控制面 API 暴露为模型工具。

## 3. 契约基线

实施对齐以下官方文档和 Codex 源码快照为基线：

- [Codex Subagents 文档](https://learn.chatgpt.com/docs/agent-configuration/subagents?surface=app)
- [Codex V2 工具注册](https://github.com/openai/codex/blob/ede5247893a50297a47c9aa5038e6ab28312ff50/codex-rs/core/src/tools/spec_plan.rs#L1131-L1190)
- [Codex V2 send/followup 语义分离](https://github.com/openai/codex/blob/ede5247893a50297a47c9aa5038e6ab28312ff50/codex-rs/core/src/tools/handlers/multi_agents_v2/message_tool.rs#L11-L23)

本项目的唯一模型工具集为：

| 工具 | 职责 |
|---|---|
| `spawn_agent` | 在当前 Agent 下创建命名子线程 |
| `list_agents` | 查询当前 root 树中的 live agents |
| `send_message` | 持久化排队消息，不触发 turn |
| `followup_task` | 追加任务；目标空闲时触发 turn，运行中时在安全边界交付 |
| `wait_agent` | 等待 caller mailbox、直接子 agent 最终通知或 root 主会话 steer |
| `interrupt_agent` | 中断目标当前 turn，线程保留且可继续使用 |

`read_agent`、`close_agent`、`send_message_to_agent` 和 `wait_agents` 不在模型工具集内。

## 4. 总体架构

```text
Root Agent Session
    |
    v
AgentControl ---------------------------------------------------+
    |                                                           |
    +--> AgentRegistry ---- canonical path / tree / quota        |
    +--> AgentMailbox ----- ordered durable communication        |
    +--> AgentRuntimeManager - resident runtime / lazy restore   |
    +--> AgentGraphStore -- edges / status events / recovery     |
                                                                |
Model tools (six V2 tools)             Desktop control plane     |
    |                                              |             |
    +--> AgentControl                              +--> read      |
                                                   +--> close     |
                                                   +--> snapshot  |
                                                   +--> events    |
                                                                |
SessionStore <----- full messages and tool timeline ------------+
```

### 4.1 `AgentControl`

每个 root session 只有一个 `AgentControl`，所有后代共享它。它是模型工具、桌面控制面和 runner 事件的唯一入口，不允许不同入口各自更改线程状态。

职责：

- 解析 agent id、相对名称和 canonical task path；
- 执行 spawn 的原子预留与配额校验；
- 协调 registry、mailbox、runtime manager 和 store；
- 将真实 runner 事件投影为持久状态和前端事件；
- 对外提供稳定的 V2 命令与桌面控制面操作。

### 4.2 `AgentRegistry`

`AgentRegistry` 维护 root 范围内的内存树状索引：

- canonical path，例如 `/root/research/citations`；
- parent/child 关系；
- thread id、session id、agent type 和执行配置；
- 最新状态及其有序事件位点；
- 身份配额、树深度和运行 turn 配额。

canonical path 在整个 root 树内唯一。路径一旦持久化不再重命名或复用，避免历史消息和 UI 链接指向新线程。

### 4.3 `AgentMailbox`

`AgentMailbox` 是线程间通信的持久化日志，不是临时 channel 的镜像。每条消息必须先落库再确认成功，并包含单调递增序号、幂等键、发件人、收件人、消息类型、触发语义和交付状态。

mailbox 支持：

- 按收件人和序号严格有序消费；
- 重试时按幂等键去重；
- 事件通知只作为唤醒信号，真实内容从持久化日志读取；
- 进程重启后从最后确认位点恢复交付。

### 4.4 `AgentRuntimeManager`

`AgentRuntimeManager` 只维护当前驻留进程的 runner、中断句柄和 turn 任务。已完成或被中断的线程可以卸载；新 follow-up 到达时，从 SessionStore 的结构化历史延迟恢复。

恢复不使用展平的文本 transcript，而是读取真实 message role、tool calls、tool results、usage 和上下文元数据。

### 4.5 `AgentGraphStore`

`AgentGraphStore` 保存树结构和控制元数据，但不复制完整对话。它与 SessionStore 的责任边界是：

- `subagents-v2.db`：线程身份、spawn edge、mailbox、状态事件、close 和恢复元数据；
- `state.db`：完整消息、工具调用与结果、usage、搜索与上下文历史。

## 5. 模型工具契约

### 5.1 `spawn_agent`

输入：

```json
{
  "task_name": "research",
  "message": "Investigate the provider failure.",
  "agent_type": "explorer",
  "model": "optional-model",
  "reasoning_effort": "high",
  "fork_turns": "all"
}
```

- `task_name` 和 `message` 必填。
- `task_name` 只允许小写字母、数字和下划线。
- `agent_type`、`model`、`reasoning_effort`、`fork_turns` 可选。
- `fork_turns` 接受 `none`、`all` 或正整数字符串。
- 子路径由当前 agent path 和 `task_name` 生成，例如 `/root/research`。
- spawn 先原子预留路径、身份配额和 spawn edge；初始化任一步失败必须回滚预留。
- 父历史按 `fork_turns` 转换为结构化 Session 输入，不得压成字符串塞进第一条用户消息。

内部 dispatch 结果保留 thread id、Session id、canonical path 和初始状态，供运行时与 Desktop 控制面使用。默认模型可见输出对齐 Codex V2 `hide_spawn_agent_metadata=true` 契约，只返回 canonical `task_name`，不泄露内部 thread/session ID。

### 5.2 `list_agents`

输入仅允许可选 `path_prefix`。查询边界是当前 root 的整棵树，不是当前 agent 的直接子节点。

结果按 canonical path 稳定排序，模型可见形状只包含 `agent_name` 和 `agent_status`。不接受 `include_closed` 参数；已 `Shutdown` 或已从 live registry 移除的线程不返回。其完整历史节点仍保留在持久化 graph 中，由 Desktop snapshot/read 控制面查询。

### 5.3 `send_message`

输入为 `target` 和 `message`。`target` 接受相对 task name、canonical task path 或 thread ID，并可以指向当前 agent 自身。操作只将消息持久化到目标 mailbox 并发布唤醒事件，不创建 turn、不恢复空闲 runtime、不改写目标状态。

当目标下次进入消息边界时，按 mailbox 序号注入待读消息。

### 5.4 `followup_task`

输入为 `target` 和 `message`。`target` 接受相对 task name、canonical task path 或 thread ID，且不得指向 root。消息先以 follow-up 类型持久化，然后根据目标实际运行状态处理：

- 目标无活动 turn：延迟恢复 runtime 并触发新 turn；
- 目标正在运行：在下一个安全消息或工具边界交付；
- 目标已 `Interrupted`：使用现有 Session 恢复为新 turn；
- 目标已 `Shutdown`：拒绝并返回明确错误。

### 5.5 `wait_agent`

输入只允许可选 `timeout_ms`，不接受 target 列表。

调用时先检查当前 caller 可见的、已存在但尚未确认的 mailbox/activity，避免在建立 wait 前已发生的活动丢失。若无待处理活动，再从当前 cursor 等待以下任一事件：

- 发送给当前 caller 的 agent-to-agent mailbox 活动；
- 当前 caller 直接子 agent 的最终或状态通知；
- root caller 主会话用户输入对当前 turn 的 steer；
- timeout。

工具返回只包含活动概要或 `timed_out`。具体消息作为独立上下文事件按序注入，避免将新内容捆绑进工具结果。

### 5.6 `interrupt_agent`

输入为 `target`，接受相对 task name、canonical task path 或 thread ID。禁止中断 root 或当前自身。

命令读取并返回目标的前一状态。目标存在活跃 turn 时发出中断信号，仅当 runner 确认 turn 终止时才持久化 `Interrupted` 事件；目标空闲、已完成或 runtime 已离线时是成功 no-op。线程、Session 和 canonical path 保留，之后可接收 follow-up。

## 6. 桌面控制面

桌面应用可使用模型不可见的管理操作：

### 6.1 Read

`read` 根据 root 和 canonical target 打开其真实 Session 时间线，包括用户消息、assistant 消息、tool calls、tool results、中断与错误事件。它不从 Agent Graph 数据库读取简化 transcript 作为对话内容。

### 6.2 Close

`close` 是桌面控制面的树操作：

1. 锁定目标子树并阻止新 spawn/follow-up；
2. 从叶子到根递归中断活动 turn；
3. 等待每个 runner 确认终止；
4. 刷新 Session 和 mailbox 位点；
5. 持久化 closed edge 与 `Shutdown` 事件。

close 必须幂等。重复关闭已 `Shutdown` 线程返回成功快照，不创建新状态分支。

## 7. 状态机

持久化状态为：

| 状态 | 含义 | 是否可继续 |
|---|---|---|
| `PendingInit` | 路径已预留，runtime/Session 正在初始化 | 否，等待初始化 |
| `Running` | 存在真实活动 turn | 可接收排队消息或追加任务 |
| `Interrupted` | 最近的 turn 已中断，线程未终结 | 是 |
| `Completed { last_message }` | 最近的 turn 正常完成 | 是 |
| `Errored { message }` | 最近的 turn 或 runtime 初始化失败 | 可按错误类型重试 |
| `Shutdown` | 线程已被控制面关闭 | 否 |

`NotFound` 只是查询时的合成结果，不写入数据库。

状态变化只能来自 runner/runtime 事件：

```text
turn_started       -> Running
turn_completed     -> Completed
turn_interrupted   -> Interrupted
turn_errored       -> Errored
runtime_terminated -> Shutdown
```

API 不得在发出 interrupt、close 或 spawn 命令后乐观写入终态。`Completed`、`Interrupted` 和 `Errored` 是 turn 结果，不代表线程不可再使用。

## 8. 配额和并发

身份配额和执行配额分离：

- **身份配额**：限制整棵树的线程数和最大深度；
- **执行配额**：只统计当前正在运行的 turn。

`Completed`、`Interrupted` 和可恢复的 `Errored` 线程仍可定址，但不占用执行槽。spawn 和 follow-up 必须在启动 turn 前获得执行许可，结束、中断、错误和初始化失败都必须通过 guard 释放许可。

## 9. 持久化模型

`~/.astro/subagents-v2.db` 是 Astro V2 Agent Graph/mailbox 数据库，并使用显式 schema version。目标表结构如下：

### 9.1 `agent_threads`

- `thread_id`
- `root_thread_id`
- `parent_thread_id`
- `canonical_path`
- `task_name`
- `agent_type`
- `session_id`
- `status_kind`
- `status_payload`
- `last_status_sequence`
- `created_at`
- `updated_at`

`canonical_path` 在 root 内唯一，`session_id` 指向 SessionStore 的真实时间线。

### 9.2 `agent_spawn_edges`

- `parent_thread_id`
- `child_thread_id`
- `edge_state` (`open` / `closed`)
- `created_at`
- `closed_at`

### 9.3 `agent_mailbox`

- `sequence`
- `message_id`
- `idempotency_key`
- `sender_thread_id`
- `recipient_thread_id`
- `kind` (`message` / `followup` / `result` / `status`)
- `payload`
- `trigger_turn`
- `delivery_state`
- `created_at`
- `delivered_at`

### 9.4 `agent_status_events`

- `sequence`
- `thread_id`
- `event_kind`
- `payload`
- `source_turn_id`
- `created_at`

`agent_threads` 中的状态是最新投影，`agent_status_events` 是可追溯日志。两者在同一事务内更新。

### 9.5 `schema_meta` 与历史归档

`schema_meta` 记录当前 schema version 和已完成的一次性迁移。旧实现的 thread/transcript 数据迁入只读历史归档，桌面端可查看，但：

- 不注册到 V2 Agent Tree；
- 不可接收 message/follow-up；
- 不可恢复 runner；
- 不占用身份或执行配额。

## 10. 一致性与故障恢复

### 10.1 Spawn 预留

spawn 使用 reservation guard，按以下顺序执行：

1. 校验 task name、路径唯一性、深度和身份配额；
2. 在事务中写入 `PendingInit` thread 和 open edge；
3. 创建 Session 和 runtime；
4. 获得执行许可并启动 turn；
5. runner 发布 `turn_started`。

任一步失败都由 guard 回滚未完成的路径、配额、edge 和 runtime 资源。如果 Session 已创建但 runtime 失败，保留可诊断 Session，thread 记录为 `Errored`，不留下执行许可。

### 10.2 Mailbox 保证

- 先持久化，后返回成功；
- 单调 sequence 保证有序交付；
- idempotency key 保证客户端重试不产生重复 turn；
- 通知丢失时，恢复扫描仍能找到未交付消息；
- 交付位点与 Session 注入成功同步提交或使用可重放的事务边界。

### 10.3 进程重启

启动时恢复 root tree、路径索引、mailbox cursor 和最新状态投影。上次进程结束时仍为 `Running` 的 turn 追加耐久 `Interrupted` 事件，但线程仍可通过 follow-up 延迟恢复。

启动恢复不自动重放未完成的 LLM 或工具调用，避免无幂等副作用。

## 11. 配置语义

自定义 agent 配置只从以下路径加载：

1. `~/.codex/agents/*.toml`
2. `<project>/.codex/agents/*.toml`

project 配置覆盖用户配置。删除以下兼容输入：

- `~/.astro/agents/*.toml`
- `<project>/.astro/agents/*.toml`
- `~/.astro/config.toml` 中的 Codex agent 兼容配置
- `<project>/.astro/config.toml` 中的 Codex agent 兼容配置

`~/.astro` 继续作为 Astro 会话、数据库、日志、workflow 和其他运行数据主目录。

自定义 agent 可配置 model、reasoning effort、sandbox 收窄、MCP 和 skill 开关；子 agent 只能收窄父权限，不得扩大文件、网络、工具或审批权限。

## 12. 前端与桌面适配

### 12.1 数据流

前端不再每 1.5 秒轮询 subagent 状态。新数据流为：

1. 打开主会话时请求 Agent Tree 快照；
2. 从现有 Session Event 通道接收 agent spawned、status changed、mailbox activity、edge closed 事件；
3. reducer 按事件 sequence 幂等投影到本地 Agent Tree；
4. 重连时先取新快照，再从快照 cursor 后应用增量事件。

### 12.2 主会话活动条

`SubagentActivityBar` 升级为可折叠 Agent Tree 活动条：

- 用层级缩进显示 canonical path 关系；
- 区分 Running、Interrupted、Completed、Errored 和 Shutdown；
- Running 只对应真实活动 turn；
- 显示未读 mailbox/final activity；
- 点击节点打开该 agent 的真实 Session 时间线。

### 12.3 管理面板

`SubagentsPanel` 保留桌面管理能力：

- 查看完整树和 Session；
- 通过 V2 control plane 发送 follow-up；
- 中断当前 turn；
- 递归关闭子树；
- 查看历史归档，但归档项不显示可执行操作。

旧模型工具名不得出现在工具目录、回退文案、i18n、示例或帮助内容中。

## 13. 硬切删除清单

实施必须直接删除，不保留 deprecated 转发层：

### 13.1 模型工具

- `read_agent`
- `close_agent`
- `send_message_to_agent`
- `wait_agents`

### 13.2 旧参数别名

- `task`
- `thread_id`
- `thread_ids`
- `include_closed`
- 其他与 V2 schema 不一致的参数名或宽松 JSON 解析分支

### 13.3 配置兼容

- `.astro/agents`
- `.astro/config.toml` 中的 agent 配置兼容加载器

### 13.4 代码与文档残留

同步更新或删除：

- tool registry 和 schema；
- prompt/tool guidance；
- toolset 名称映射；
- interaction mode 引导；
- context usage 分组；
- Tauri/proto 控制面；
- 前端 fallback catalog 和 i18n；
- `docs/subagents.md`、`AGENTS.md` 和其他开发者文档；
- 依赖旧名称或旧参数的单元与集成测试。

## 14. 迁移策略

代码契约硬切与数据保全分开执行：

1. 先引入新 schema 和一次性迁移器；
2. 将旧 thread/transcript 记录复制到历史归档并标记 migration version；
3. 创建新 V2 空 Agent Graph，不将旧 Running/Pending 状态恢复为可执行任务；
4. 切换模型工具注册和运行时；
5. 切换桌面控制面和前端事件流；
6. 删除所有旧工具、参数、轮询与配置兼容代码；
7. 用静态搜索和契约测试证明旧入口不再存在。

迁移器必须幂等，重复启动不能重复归档或引入重复记录。

## 15. 测试与验收

### 15.1 单元测试

- AgentPath 解析、相对名称解析和 canonical path 稳定性；
- 重复 task name、非法 task name、最大树深度和身份配额；
- spawn reservation 在 Session/runtime/turn 各失败点的回滚；
- `send_message` 只排队不启动 turn；
- `followup_task` 在空闲、运行、Interrupted 和 Shutdown 状态下的行为；
- `wait_agent` 分别由 mailbox、result/status、main steer 和 timeout 唤醒；
- `interrupt_agent` 返回前一状态，并且只在 runner 确认后进入 `Interrupted`；
- `Interrupted` 不是终态，follow-up 可恢复；
- 嵌套 list、递归 close 和执行配额释放；
- mailbox 顺序、幂等去重和未交付恢复。

### 15.2 迁移与重启测试

- 旧 schema 只迁移一次；
- 历史归档可读但不可执行；
- 重启恢复树结构、mailbox cursor 和事件顺序；
- 崩溃前 `Running` 在恢复时转为耐久 `Interrupted`；
- lazy runtime load 使用真实 Session 消息和 tool timeline；
- 重启后未交付 follow-up 不丢失且不重复启动 turn。

### 15.3 契约测试

- registry 仅暴露 6 个 V2 工具；
- 旧四个模型工具名均不存在；
- 旧参数别名被 schema 拒绝；
- `read` 和 `close` 只出现在桌面控制面；
- `.astro/agents` 和 `.astro/config.toml` 不再影响 agent 配置；
- project `.codex/agents` 正确覆盖用户 `.codex/agents`；
- 子 agent 配置无法扩大父权限。

### 15.4 前端测试

- 快照能构建多层 Agent Tree；
- 增量事件按 sequence 幂等更新；
- 断线重连通过新快照 + cursor 不丢状态；
- 活动条只将真实 turn 显示为 Running；
- 打开 agent 显示真实 Session 时间线；
- interrupt、follow-up 和递归 close 通过控制面生效；
- 历史归档不显示可执行操作。

### 15.5 最终验证命令

```bash
cargo fmt --all --check
cargo test -p subagents
cargo test -p tools
cargo test -p agent
cargo test -p server
cargo check --workspace --all-targets
cargo test --workspace
cd apps/desktop && npm test
cd apps/desktop && npx tsc --noEmit
cd apps/desktop && npm run build
git diff --check
```

## 16. 验收标准

以下条件全部满足时，本重构才视为完成：

1. 模型工具集严格只有 6 个 Codex V2 工具，且参数 schema 不接受旧别名。
2. 任意深度的 agent 都能使用 canonical path 在同一 root tree 内被查询和定址。
3. `send_message` 不触发 turn，`followup_task` 按目标活动状态正确触发或交付。
4. `wait_agent` 在任一 mailbox/final/steer 活动时唤醒，不再是“轮询一组 id 直到全部终态”。
5. interrupt 不关闭线程，close 仅由桌面控制面递归执行。
6. 线程状态只由真实 runner 事件推导，所有资源在失败路径上正确释放。
7. 前端使用快照 + 增量事件，主会话内实时展示 Agent Tree，并能打开真实 Session。
8. 旧数据可在历史归档查看，但旧运行时、旧工具和旧配置入口已完全删除。
9. 聚焦测试、workspace all-targets 检查、前端测试、TypeScript 检查和生产构建全部通过。
