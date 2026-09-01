# Hook 配置样例

把下列片段放到数据根（默认 `~/.astro`，可用 `ASTRO_MEMORY_DIR` 覆盖）。

Command/MCP hooks 使用 `hooks.json` 或 `config.toml`，可以参与 typed lifecycle 决策；本页原有 `config.yaml` 示例属于 legacy Shell telemetry，只观察事件、不参与控制流。完整 command/MCP 示例见 [Hook 运行契约](../../hooks.md#5-配置与信任)。

## Shell Hooks — `config.yaml`

```yaml
# ~/.astro/config.yaml
hooks:
  PreGatewayDispatch: 'mkdir -p "$HOME/.astro/logs" && echo "$(date -Iseconds) event=$ASTRO_HOOK_EVENT session=$ASTRO_HOOK_SESSION" >> "$HOME/.astro/logs/chat-audit.log"'
  CommandNewChat: 'true'
  GatewayStartup: 'true'
```

Hook key 必须使用 canonical 名称。由 Session runtime 触发的 `PostToolUse`、`PreLlmCall`、`PostLlmCall` 与 `AgentEnd` 都会经 `HookRuntime` 投递给 Shell；仍应只为实际 fire 的事件配置命令。

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
| `ASTRO_TELEMETRY_INCLUDE_DETAIL` | 可选；仅设为 `1` 时发送 hook detail，默认留空 |

`config.yaml.snippet` 中，`GatewayStartup` 的 telemetry command 只需取消注释；`PreGatewayDispatch` 和 `CommandNewChat` 已有主示例 command，需将它们的 command 替换为 `telemetry-webhook.sh`。这些事件以及 Session runtime 的 `PostToolUse`、`PreLlmCall`、`PostLlmCall`、`AgentEnd` 都可实际投递到 Shell。

该脚本发送 Astro 自定义 JSON。请使用自建 webhook，或自行适配后再转发至 Langfuse、OpenTelemetry 等后端；它不声明这些后端的原生 ingestion 协议兼容。

**安全提示**

- 勿在日志或 echo 中打印 `ASTRO_TELEMETRY_TOKEN`。
- `ASTRO_HOOK_DETAIL` 可能包含 prompt 原文、工具参数或工具结果预览；脚本默认不发送，确认接收端安全后才设置 `ASTRO_TELEMETRY_INCLUDE_DETAIL=1`。
- `ASTRO_HOOK_MESSAGE` 可能包含 prompt 或 assistant 文本，勿盲目上传到第三方。
- `tool_input` / `tool_response` 不会写入 env。如需更完整上下文，请在自有端点侧按 `session_id` + `turn_id` 关联本地 `agent.log`。

完整说明：[docs/hooks.md](../../hooks.md)
