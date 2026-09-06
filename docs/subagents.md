# Subagents: Codex V2 Agent Threads

Astro 只有一套 Subagent 运行时契约：持久化的 Agent Thread 树。每个节点有独立 Session 时间线，共享 root-scoped 控制面；不存在一次性 delegate、resident channel、Team/pipeline 兼容层或隐式 git worktree。

## 模型工具契约

模型仅能调用六个 Agent Thread 工具：

- `spawn_agent(task_name, message, agent_type?, model?, reasoning_effort?, fork_turns?)`：在当前路径下创建子线程并启动首个 turn。`fork_turns` 接受 `none`、`all` 或正整数。
- `list_agents(path_prefix?)`：按 canonical path 稳定排序列出 root 树中的 live agents；已 `Shutdown` 节点不进入模型结果。
- `send_message(target, message)`：只持久化到目标 mailbox，不启动新 turn。
- `followup_task(target, message)`：持久化 mailbox；目标运行中时在安全边界交付，空闲或已中断时触发/恢复下一个 turn。
- `wait_agent(timeout_ms?)`：先检查已存在的未处理活动，再等待 mailbox、直接后代最终通知或主会话 steer，而不是轮询某组 thread id。
- `interrupt_agent(target)`：中断目标当前活跃 turn；目标空闲或已结束时是 no-op，线程仍可被 follow-up 恢复。

`send_message`、`followup_task` 和 `interrupt_agent` 的 `target` 均接受相对 task name、canonical task path 或 `spawn_agent` 对应的 thread ID。`send_message` 可向当前 agent 自身排队；`followup_task` 不得目标 root，`interrupt_agent` 不得目标 root 或当前 agent。

Root turn 的 `service_tier` 由 root-scoped `AgentControl` 共享。`spawn_agent`、嵌套
spawn 和后续启动的 Subagent turn 都从该快照继承 tier；只有 OpenAI/Codex
backend 会将它写入 Provider 参数，其他 backend 不透传不支持的字段。模型、
reasoning effort 和 sandbox 的现有继承/收窄规则不变。`AgentTreeSnapshotV2` 同时
携带 nullable `root_service_tier`，Desktop 在 Agent Tree 摘要中展示实际根级值；
增量状态事件与 mark-read 投影不会清除它。详见
[Agent Tree 状态投影详细设计](04-详细设计阶段/01-核心引擎层/15-Agent-Tree状态投影详细设计.md)。

模型可见输出保持 Codex V2 紧凑形状：`spawn_agent` 默认只返回 canonical `task_name`，`list_agents` 只返回 `agent_name` 和 `agent_status`，不泄露内部 thread/session ID。完整身份只在运行时与 Desktop 控制面中使用。

`read` 和递归 `close` 仅属于桌面管理控制面。Tauri 命令 `read_subagent_thread` 读取真实 Session 时间线，`close_subagent_thread` 按叶子优先终止目标子树。它们不是模型工具，也不经过模型 dispatch trait。

## 生命周期与恢复

线程只有六种状态：`PendingInit`、`Running`、`Interrupted`、`Completed`、`Errored`、`Shutdown`。

- `Running` 仅表示真实活跃 turn，turn 结束就释放执行配额，线程身份仍保留。
- `Interrupted` 和 `Completed` 都可通过 `followup_task` 再次启动。
- `Shutdown` 是桌面 close 后的不可执行历史节点。
- 进程重启时，未完成的 `Running` turn 投影为耐久 `Interrupted`；不自动重放 LLM 或工具副作用。
- `SubagentStart` 只在首轮 startup-ready 且 caller 接受后触发一次；`SubagentStop` 只在 Desktop close 耐久化 `Shutdown` 并成功收敛后触发一次。两者都是进程内观察回调，不跨进程重启回放。

通知只用于唤醒。状态事件、mailbox sequence 和投递位点先持久化，Session Event 再以 `stream_id + event_id` 按 cursor 回放。前端先取快照，再应用 cursor 之后的增量事件。

## 配置与权限

自定义 Agent 只从以下路径加载，项目定义覆盖用户定义：

- `~/.astro/agents/*.toml`
- `<project>/.astro/agents/*.toml`（仅可信项目）

```toml
name = "reviewer"
description = "Reviews changes for defects and missing tests."
developer_instructions = "Stay read-heavy. Report findings with concrete evidence."
model = "openai:gpt-5.6"
model_reasoning_effort = "high"
sandbox_mode = "read-only"

[mcp_servers.openaiDeveloperDocs]
url = "https://developers.openai.com/mcp"

[[skills.config]]
path = "/absolute/path/to/SKILL.md"
enabled = true
```

`name`、`description`、`developer_instructions` 必填。自定义 sandbox 只能收窄父任务权限；MCP 与 skill 也不得扩大父任务的文件、网络、工具或审批权限。内置 agent 为 `default`、`worker`、`explorer`。

全局与项目设置也只从 `~/.astro/config.toml` 和可信项目的
`<project>/.astro/config.toml` 加载；`.codex` 不作为 Astro 配置输入：

```toml
[agents]
enabled = true
max_concurrent_threads_per_session = 4
default_subagent_model = "openai:gpt-5.6"
default_subagent_reasoning_effort = "high"
interrupt_message = true
```

## 持久化边界

- `~/.astro/data/subagents-v2.db`：V2 Agent Graph、spawn edge、mailbox、状态事件和恢复元数据。
- `~/.astro/data/state.db`：每个 Agent Thread 的真实消息、reasoning、tool call/result 时间线。

旧 V1 schema 只能被一次性迁移器识别：原 thread/message 表被改名为只读历史归档，不会恢复为可执行 runtime，也不提供旧模型 API。
