# Subagents: Codex-style Agent Threads

Astro 的 Subagent 只有一套运行时语义：持久化的 Agent Thread。旧的 `subagent` 一次性委派、`pipeline` / Team 编排、嵌套深度门禁和自动 git worktree 已移除。

## 工具

- `spawn_agent`：启动独立线程，`fork_turns` 支持 `none` / `all` / 正整数。
- `list_agents` / `read_agent`：查看父会话的线程及其消息。
- `send_message_to_agent`：在下一个模型边界给线程追问或 steer。
- `wait_agents`：等待指定线程完成当前回合并返回摘要。
- `interrupt_agent`：中断当前 LLM / tool 回合。
- `close_agent`：关闭线程并释放进程内控制句柄。

线程状态与对话写入 `~/.astro/subagents.db`。Provider 凭证仅在 spawn 时以内存值传递，不写入该数据库。子线程完整继承父任务权限，自定义 agent 不能扩大 sandbox 权限。

## 自定义 Agent

定义从两层目录加载，项目层同名 agent 覆盖个人层和内置定义：

- 个人：`~/.astro/agents/*.toml`
- 项目：`<project>/.astro/agents/*.toml`

```toml
name = "reviewer"
description = "Reviews changes for defects and missing tests."
developer_instructions = "Stay read-heavy. Report findings with concrete evidence."
model = "openai:gpt-5.6"
model_reasoning_effort = "high"
```

`name`、`description`、`developer_instructions` 必填。内置 agent 为 `default`、`worker`、`explorer`。

## 全局配置

`~/.astro/config.toml` 和 `<project>/.astro/config.toml` 支持：

```toml
[agents]
enabled = true
max_concurrent_threads_per_session = 4
default_subagent_model = "openai:gpt-5.6"
default_subagent_reasoning_effort = "high"
interrupt_message = true
```

聊天输入框的 Subagents 按钮可直接检查线程、发送 follow-up、中断或关闭线程。
