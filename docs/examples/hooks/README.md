# Hook 配置样例

把下列片段放到数据根（默认 `~/.astro`，可用 `ASTRO_MEMORY_DIR` 覆盖）。

## Shell Hooks — `config.yaml`

```yaml
# ~/.astro/config.yaml
hooks:
  PreGatewayDispatch: 'mkdir -p "$HOME/.astro/logs" && echo "$(date -Iseconds) event=$ASTRO_HOOK_EVENT session=$ASTRO_HOOK_SESSION" >> "$HOME/.astro/logs/chat-audit.log"'
  CommandNewChat: 'true'
  GatewayStartup: 'true'
```

迁移对照（不要把新旧 key 同时写入配置）：

```text
post_tool_call  -> PostToolUse
agent:end       -> AgentEnd
on_session_end -> AgentEnd
pre_llm_call    -> PreLlmCall
```

上述四行只是命名迁移对照。`PostToolUse` / `PreLlmCall` 及 AgentLoop 中直接 fire 的 `AgentEnd` 当前不会经 `HookRuntime` 自动投递给 Shell，不要把它们当作 Batch A 的可运行 Shell 示例。

## Gateway Event Hooks — `hooks/<name>/HOOK.yaml`

```text
~/.astro/hooks/audit/HOOK.yaml
```

内容见同目录 `HOOK.yaml` 样例文件。未在代码中 `register_gateway_handler` 时，Astro 会自动挂默认 tracing 日志 handler。

## Telemetry webhook

将 [`telemetry-webhook.sh`](./telemetry-webhook.sh) 复制到数据根 hooks 目录并赋予执行权限：

```bash
cp docs/examples/hooks/telemetry-webhook.sh ~/.astro/hooks/
chmod +x ~/.astro/hooks/telemetry-webhook.sh
```

在 shell 或 `~/.astro/config.yaml` 同进程环境中设置：

| 变量 | 说明 |
|------|------|
| `ASTRO_TELEMETRY_URL` | 接收 JSON POST 的端点；未设置时脚本静默 `exit 0` |
| `ASTRO_TELEMETRY_TOKEN` | 可选；若设置则作为 `Authorization: Bearer …` 发送 |

`config.yaml.snippet` 中，`GatewayStartup` 的 telemetry command 只需取消注释；`PreGatewayDispatch` 和 `CommandNewChat` 已有主示例 command，需将它们的 command 替换为 `telemetry-webhook.sh`。这三个事件当前都经过 `HookRuntime`，可实际投递到 Shell。

**安全提示**

- 勿在日志或 echo 中打印 `ASTRO_TELEMETRY_TOKEN`。
- `ASTRO_HOOK_DETAIL` 仅为摘要；`ASTRO_HOOK_MESSAGE` 可能包含 prompt 或 assistant 文本，勿盲目上传到第三方。
- `tool_input` / `tool_response` 不会写入 env。如需更完整上下文，请在自有端点侧按 `session_id` + `turn_id` 关联本地 `agent.log`。

完整说明：[docs/hooks.md](../../hooks.md)
