# Astro Hook 体系

Astro 提供与常见 Agent 生命周期对齐的 **三套 Hook**：

1. **Plugin Hooks** — Agent 推理循环内部（`pre_llm_call`、`pre_tool_call` …）
2. **Gateway Event Hooks** — 会话/入口外壳（`gateway:startup`、`session:start`、`agent:end` …）
3. **Shell Hooks** — `~/.astro/config.yaml` 里声明的 shell 命令（旁路、异步）

进程内用 Rust 注册：`ctx.register_hook("post_tool_call", callback)`。不加载第三方动态库。

设计规格见 [`docs/superpowers/specs/2026-07-14-hooks-system-design.md`](./superpowers/specs/2026-07-14-hooks-system-design.md)。  
可复制的配置样例：[docs/examples/hooks/](./examples/hooks/)。

---

## 1. Plugin Hooks（Agent 生命周期）

### 钩子表

| 钩子名 | 触发时机 | 可否影响流程 |
|--------|----------|--------------|
| `on_session_start` | 该 session 首次进入主循环 | 观察 |
| `pre_llm_call` | 每轮 LLM 前（一次） | 可 `InjectContext` |
| `pre_api_request` | 每次底层 API 调用前 | 观察 |
| `post_api_request` | 每次 API 结束后 | 观察 |
| `pre_tool_call` | 工具执行前 | `Continue` / `Block` / `Modify(args)` |
| `post_tool_call` | 工具返回后 | 观察 |
| `post_llm_call` | 该 turn 成功结束后 | 观察 |
| `on_session_end` | 单次 stream/run 收尾 | 观察 |
| `on_session_finalize` | 卸会话 / 进程清理 | 观察 |
| `on_session_reset` | 新建对话切走旧 session | 观察 |
| `subagent_stop` | `delegate` 子 Agent 结束后（父侧） | 观察 |
| `pre_gateway_dispatch` | gRPC/Tauri chat 入站前 | `Allow` / `Skip` / `Rewrite` |

顺序：

```text
on_session_start（仅首轮）
  → pre_llm_call
  → [工具循环]
      → pre_api_request → API → post_api_request
      → pre_tool_call → 工具 → post_tool_call
  → post_llm_call
  → on_session_end
```

### 注册示例

```rust
use hooks::{HookOutcome, HookPayload, HookRuntime, PluginContext, POST_TOOL_CALL};

fn register(ctx: &PluginContext<'_>) {
    ctx.register_hook(POST_TOOL_CALL, |payload: &HookPayload| {
        tracing::info!(tool = ?payload.tool_name, "tool finished");
        HookOutcome::Continue
    });
}

let rt = HookRuntime::bootstrap_from_root(&std::path::PathBuf::from("/tmp/astro")).unwrap();
register(&rt.plugin_context());
```

可拦截示例：

```rust
ctx.register_hook("pre_tool_call", |p| {
    if p.tool_name.as_deref() == Some("terminal") {
        return HookOutcome::Block("terminal disabled by policy".into());
    }
    HookOutcome::Continue
});

ctx.register_hook("pre_llm_call", |_| {
    HookOutcome::InjectContext("User timezone: Asia/Shanghai".into())
});
```

`InjectContext` 只影响**本轮**送给模型的消息视图（追加 `[astro:hook-context]`），不改写数据库里的用户原文。

---

## 2. Gateway Event Hooks

目录：`~/.astro/hooks/<name>/HOOK.yaml`

```yaml
name: audit
description: 会话审计
events:
  - gateway:startup
  - session:start
  - agent:end
  - command:new_chat
```

Handler 可在进程内绑定（按清单 `name`）：

```rust
ctx.register_gateway_handler("audit", |event, payload| {
    tracing::info!(%event, session = %payload.session_id, "gateway hook");
});
```

若未自定义绑定，启动时会为已发现清单自动安装 **tracing 日志 fallback**（仅放置 `HOOK.yaml` 即可生效）。

| 事件 | 触发点 |
|------|--------|
| `gateway:startup` | backend 启动完成 |
| `session:start` | 新 session 首次 chat |
| `agent:end` | 一次 chat run 收尾 |
| `command:new_chat` | UI 新建对话（`ChatControl` / `new_chat`）→ 卸内存会话，并触发 `on_session_reset` / `on_session_finalize` |

---

## 3. Shell Hooks

`~/.astro/config.yaml`（数据根可用 `ASTRO_MEMORY_DIR` 覆盖）：

```yaml
hooks:
  post_tool_call: 'echo "$ASTRO_HOOK_TOOL" >> "$HOME/.astro/logs/tool-audit.log"'
  agent:end: 'true'
```

环境变量：`ASTRO_HOOK_EVENT`、`ASTRO_HOOK_SESSION`、`ASTRO_HOOK_DETAIL`、`ASTRO_HOOK_TOOL`、`ASTRO_HOOK_MESSAGE`。

- 异步执行，默认超时 5 秒  
- 失败只记日志  
- 与 Plugin 同名事件可同时触发（Shell 始终旁路）

---

## 4. UI

- 流事件：`ChatEvent.hook`（`name` / `detail` / `outcome`），**不再**经 `memory_update` 伪装  
- 前端活动卡 `kind: "hook"`，受设置「Hook 事件」开关控制（`normal` / `detailed` 预设默认开启）  
- 标题为钩子名，例如 `pre_llm_call`  
- UI「新建对话」会经 `chat_control(new_chat)` 触发 Gateway `command:new_chat` 并卸内存会话

---

## 5. Crate 地图

| Crate / 模块 | 职责 |
|--------------|------|
| `hooks` | `PluginHookBus`、`GatewayHookRegistry`、`ShellHookRunner`、`HookRuntime` |
| `agent::hooks` | `PromptHooks` trait、`ChannelHooks`、`RecordingHooks`（观察推送）；可拦截走 `PluginHookBus` |
| `backend` | 启动扫描、`pre_gateway_dispatch`、Gateway 事件、`new_chat` 卸会话 |
| `permissions` | 危险命令检测与权限裁决（钩子已迁出） |

---

## 6. 测试

```bash
cargo test -p hooks
cargo test -p agent --test streaming_test multi_turn_fires_post_llm_call
cargo test -p agent --test rig_agent_test test_prompt_hooks_on_run_turn
cargo test -p agent --test rig_agent_test pre_tool_call_block_via_hook_bus
cargo test -p agent --test rig_agent_test pre_llm_call_inject_context_via_hook_bus
```
