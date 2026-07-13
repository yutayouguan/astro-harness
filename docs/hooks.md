# Astro Hooks

Astro 提供三套钩子体系（行为对齐 Hermes 生命周期语义，产品文案不使用 hermes 品牌字样）：

| 体系 | 用途 |
|------|------|
| **Plugin Hooks** | Agent 会话生命周期：LLM / 工具 / 会话起止 |
| **Gateway Event Hooks** | 入站消息、会话命令、启动扫描 |
| **Shell Hooks** | 配置驱动的外部脚本旁路执行 |

进程内注册入口：`hooks::PluginContext::register_hook(name, callback)`（见 `hooks` crate）。

## Plugin 钩子名

| 名称 | 时机 |
|------|------|
| `on_session_start` | 会话 / run 开始 |
| `pre_llm_call` | 组装 system prompt 后、请求模型前（可 `InjectContext`） |
| `pre_api_request` / `post_api_request` | Provider HTTP/API 调用前后 |
| `pre_tool_call` / `post_tool_call` | 工具执行前后（可 `Block` / `Modify`） |
| `post_llm_call` | 助手回复聚合后 |
| `on_session_end` | 本轮 multi-turn 结束 |
| `on_session_finalize` / `on_session_reset` | 会话落盘 / 新会话 |
| `subagent_stop` | 子 Agent 结束 |

## 配置（Shell）

用户配置（默认 `~/.astro/config.yaml`，受 `ASTRO_MEMORY_DIR` 影响）：

```yaml
hooks:
  pre_tool_call:
    - command: ["echo", "tool starting"]
      timeout_secs: 5
```

Gateway 目录钩子：`~/.astro/hooks/*/HOOK.yaml`。

## UI

偏好设置中的「Hook 事件」开关（`showHooks`）控制聊天时间线是否显示生命周期钩子。  
事件经 gRPC `ChatEvent.hook`（`HookEvent { name, detail, outcome }`）下发，**不再**伪装为 `memory_update`。

## 开发

- Crate：`hooks/`（`PluginHookBus`、Gateway、Shell、UiTimeline）
- Agent 适配：`agent/src/hooks.rs`（`PromptHooks` trait + `ChannelHooks`）
- 设计：`docs/superpowers/specs/2026-07-14-hooks-system-design.md`
- 计划：`docs/superpowers/plans/2026-07-14-hooks-system.md`
