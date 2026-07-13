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

完整说明：[docs/hooks.md](../hooks.md)
