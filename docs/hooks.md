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
| `pre_approval_request` | 危险 `terminal` 命令即将进入 `Ask`（辅模型降级前） | 观察 |
| `post_approval_response` | 该次审批决议后（`auto`/`allow`/`deny`/`timeout`/`unavailable`） | 观察 |
| `transform_terminal_output` | `terminal` 原始 stdout/stderr 后、64KiB 截断前 | `ReplaceText` |
| `transform_tool_result` | 任意工具返回后、`post_tool_call` 前 | `ReplaceText` |
| `post_tool_call` | 工具返回后（已应用 `transform_tool_result`） | 观察 |
| `subagent_start` | `delegate` 子 Agent 构造完、真正 `run` 前（每个 child 一次，父侧） | 观察 |
| `subagent_stop` | `delegate` 子 Agent 结束后（父侧） | 观察 |
| `pre_verify` | 无工具调用的最终回复，且本轮执行过写盘工具（`terminal`；`file_ops` 的 `write`/`append`/`delete`/`mkdir`） | `KeepGoing(msg)` |
| `transform_llm_output` | 最终 assistant 文本定稿、`post_llm_call` 前 | `ReplaceText` |
| `post_llm_call` | 该 turn 成功结束后（已应用 `transform_llm_output`） | 观察 |
| `on_session_end` | 单次 stream/run 收尾 | 观察 |
| `on_session_finalize` | 卸会话 / 进程清理 | 观察 |
| `on_session_reset` | 新建对话切走旧 session | 观察 |
| `pre_gateway_dispatch` | gRPC/Tauri chat 入站前 | `Allow` / `Skip` / `Rewrite` |

顺序：

```text
on_session_start（仅首轮）
  → pre_llm_call
  → [工具循环]
      → pre_api_request → API → post_api_request
      → pre_tool_call
          → (pre_approval_request → 降级/park → post_approval_response)?  ← 仅危险命令走 Ask
          → 工具执行 → transform_terminal_output?（仅 terminal，截断前） → transform_tool_result → post_tool_call
          → (subagent_start … subagent_stop)?                             ← 仅 delegate/multi_agent
  → pre_verify?（本轮写盘 && attempt < 2 时才 fire；KeepGoing 则注入提示、再进 API 循环）
  → transform_llm_output → post_llm_call
  → on_session_end
```

### `ReplaceText` / `KeepGoing`

`HookOutcome` 除既有的 `Continue` / `Block` / `Modify` / `InjectContext` / `Allow` / `Skip` / `Rewrite` 外，新增两个用于上表新钩子：

| 变体 | 适用钩子 | 语义 |
|------|----------|------|
| `ReplaceText(String)` | `transform_tool_result`、`transform_terminal_output`、`transform_llm_output` | 用新字符串整体替换对应文本，再交给下游（截断 / `post_*_call` / 记录会话） |
| `KeepGoing(String)` | `pre_verify` | 不结束本 turn：把 `String` 作为附加提示注入下一轮，并回到 API 循环重新请求模型 |

短路规则与既有 mutating 钩子一致：**同名钩子按注册顺序触发，首个非 `Continue`/`Allow` 的返回值生效**（其余多回调的返回值被忽略）；`is_mutating_hook` 已把上述新钩子（含三个 `transform_*` 与 `pre_verify`）纳入「可影响流程」名单。

`pre_verify` 的重试上限为 `MAX_VERIFY_ATTEMPTS = 2`（agent 侧常量，含首次结束尝试）：超过后即使仍写过盘也不再 fire，直接进入 `transform_llm_output` → `post_llm_call` 收尾，避免死循环。写盘标记（`turn_wrote_disk`）在每次 `begin_user_turn` 清零，仅 `terminal` 与 `file_ops` 的写类 `operation` 会置位。

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

环境变量：`ASTRO_HOOK_EVENT`、`ASTRO_HOOK_SESSION`、`ASTRO_HOOK_TURN`（有值才设置）、`ASTRO_HOOK_DETAIL`、`ASTRO_HOOK_TOOL`、`ASTRO_HOOK_MESSAGE`。

- 异步执行，默认超时 5 秒  
- 失败只记日志  
- 与 Plugin 同名事件可同时触发（Shell 始终旁路）

### 遥测旁路（Shell）

示例脚本 [`docs/examples/hooks/telemetry-webhook.sh`](./examples/hooks/telemetry-webhook.sh) 将 Hook 事件 POST 到 `ASTRO_TELEMETRY_URL`（可选 `ASTRO_TELEMETRY_TOKEN`）。未配置 URL 时静默跳过，curl 失败也不阻断 agent。

`ASTRO_HOOK_TURN` 对应 `HookPayload.turn_id`（会话内流式回合 id，与 `UsageDb` / `agent.log` 一致）。这与 `HookPayload.turn`（**轮次**计数，1、2、3…）不同——后者不写入 Shell 环境变量。

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
| `agent::prompt::hooks` | `CancelSignal`；生命周期观察/拦截走共享 `PluginHookBus` + `UiTimelineSlot` |
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

# Hermes parity（7 个新钩子）
cargo test -p agent --test rig_agent_test transform_tool_result_replaces_before_post_tool_call
cargo test -p tools transform_terminal_output_hook_replaces_before_truncation
cargo test -p agent --test streaming_test transform_llm_output_replaces_before_post_llm_call
cargo test -p agent --test streaming_test approval_hooks_fire_pre_then_post_on_allow
cargo test -p agent --test streaming_test approval_hooks_fire_pre_then_post_on_deny
cargo test -p agent subagent_start_fires_before_subagent_stop_per_child
cargo test -p agent subagent_start_skips_silently_without_hook_bus
cargo test -p agent --test streaming_test pre_verify_never_fires_without_disk_write
cargo test -p agent --test streaming_test pre_verify_keep_going_retries_capped_at_two
```

---

## 7. 遥测旁路导出

每条 `HookPayload` 都携带 `session_id` 与 `turn_id`（与 `UsageDb` / `agent.log` 里的 turn_id 一致），Shell Hook 会通过 `ASTRO_HOOK_SESSION` / `ASTRO_HOOK_TURN` 环境变量暴露给外部脚本。

### 快速接入 Langfuse / 自建 Webhook

1. 复制示例脚本并赋予执行权限：

```bash
cp docs/examples/hooks/telemetry-webhook.sh ~/.astro/hooks/
chmod +x ~/.astro/hooks/telemetry-webhook.sh
```

2. 在 `~/.astro/config.yaml` 注册 Shell Hook：

```yaml
hooks:
  post_tool_call:  '"$HOME/.astro/hooks/telemetry-webhook.sh"'
  post_llm_call:   '"$HOME/.astro/hooks/telemetry-webhook.sh"'
  on_session_end:  '"$HOME/.astro/hooks/telemetry-webhook.sh"'
```

3. 设置目标端点：

```bash
export ASTRO_TELEMETRY_URL=https://your-endpoint/ingest
export ASTRO_TELEMETRY_TOKEN=sk-...   # 可选
```

脚本未设 `ASTRO_TELEMETRY_URL` 时静默 `exit 0`，不影响 agent。Shell 执行失败只记 `tracing::warn`，**不**阻断主循环。

### 环境变量一览

| 变量 | 说明 |
|------|------|
| `ASTRO_HOOK_EVENT` | 事件名（如 `post_tool_call`） |
| `ASTRO_HOOK_SESSION` | 会话 ID |
| `ASTRO_HOOK_TURN` | turn_id（会话内唯一，仅当有值时设置） |
| `ASTRO_HOOK_TOOL` | 工具名（`pre/post_tool_call` 时有值） |
| `ASTRO_HOOK_DETAIL` | 事件摘要文本 |
| `ASTRO_HOOK_MESSAGE` | 消息内容（Gateway 事件时有值） |

> **安全提示**：`tool_args` / `tool_result` 不写入 env，避免命令行泄露 API 密钥或用户数据。如需完整 payload，可在脚本内读取 `agent.log` 中对应 `turn_id` 的行。
