# Subagents: Codex-style Agent Threads

Astro 的 Subagent 只有一套运行时语义：持久化的 Agent Thread。旧的 `subagent` 一次性委派、`pipeline` / Team 编排、嵌套深度门禁和自动 git worktree 已移除。

## 工具

- `spawn_agent`：启动独立线程，`fork_turns` 支持 `none` / `all` / 正整数。
- `list_agents` / `read_agent`：查看父会话的线程及其消息。
- `followup_task` / `send_message`：Codex 兼容名称，在下一个模型边界追问或 steer。
- `wait_agent`：等待指定线程（或全部活动线程）完成当前回合并返回摘要。
- `interrupt_agent`：中断当前 LLM / tool 回合。
- `close_agent`：关闭线程并释放进程内控制句柄。

旧名称 `send_message_to_agent` / `wait_agents` 作为兼容别名保留。

线程状态与对话写入 `~/.astro/subagents.db`。Provider 凭证仅在 spawn 时以内存值传递，不写入该数据库。子线程完整继承父任务权限，自定义 agent 不能扩大 sandbox 权限。

## 自定义 Agent

Codex 路径是规范配置层；`.astro` 同名路径作为旧版兼容层保留。后加载的项目
`.codex` 定义优先级最高：

- 个人：`~/.codex/agents/*.toml`
- 项目：`<project>/.codex/agents/*.toml`
- 兼容：`~/.astro/agents/*.toml`、`<project>/.astro/agents/*.toml`

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

`name`、`description`、`developer_instructions` 必填。自定义 sandbox 只能收窄父任务权限；
`mcp_servers` 作为父配置之上的 Server 覆盖层，`skills.config` 只在子线程内生效，不修改
父 Agent 的持久化开关。内置 agent 为 `default`、`worker`、`explorer`。

## 全局配置

`~/.codex/config.toml` 和 `<project>/.codex/config.toml` 是规范路径；对应 `.astro`
路径继续兼容。支持：

```toml
[agents]
enabled = true
max_concurrent_threads_per_session = 4
default_subagent_model = "openai:gpt-5.6"
default_subagent_reasoning_effort = "high"
interrupt_message = true
```

主会话输入框上方实时显示活动 Subagent，可展开状态、打开单个线程或停止全部活动线程。
聊天输入框的 Subagents 按钮仍可打开完整面板，发送 follow-up、中断或关闭线程。
