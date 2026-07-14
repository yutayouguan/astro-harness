# Hook 配置样例

把下列片段放到数据根（默认 `~/.astro`，可用 `ASTRO_MEMORY_DIR` 覆盖）。

## Shell Hooks — `config.yaml`

```yaml
# ~/.astro/config.yaml
hooks:
  post_tool_call: 'mkdir -p "$HOME/.astro/logs" && echo "$(date -Iseconds) $ASTRO_HOOK_TOOL" >> "$HOME/.astro/logs/tool-audit.log"'
  agent:end: 'true'
  pre_llm_call: 'true'
```

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

`config.yaml.snippet` 末尾有注释掉的 `hooks:` 样例，取消注释即可挂到 `post_tool_call` / `post_llm_call` / `on_session_end`。

**安全提示**

- 勿在日志或 echo 中打印 `ASTRO_TELEMETRY_TOKEN`。
- `ASTRO_HOOK_DETAIL` 仅为摘要；勿把完整用户 prompt / 工具参数上传到第三方（`tool_args` 不会写入 env）。如需更完整上下文，请在自有端点侧按 `session_id` + `turn_id` 关联本地 `agent.log`。

完整说明：[docs/hooks.md](../../hooks.md)
