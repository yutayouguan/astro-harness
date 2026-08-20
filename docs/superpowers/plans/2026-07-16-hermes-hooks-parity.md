# Hermes Hooks Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 Astro 补齐 Hermes 有而本地缺失的 7 个 Plugin Hooks（`pre_verify`、`subagent_start`、审批对、三个 `transform_*`）。

**Architecture:** 扩展 `HookOutcome`（`ReplaceText` / `KeepGoing`）与钩子名常量；在 `runtime` / `streaming` / `delegate` / `tools_exec`（及必要时 `terminal`）现有路径插入 fire；UI 白名单同步；`pre_verify` 在写盘后的无工具终态可再进一轮 API（attempt≤2）。

**Tech Stack:** Rust workspace、`hooks` / `agent` / `tools`、现有 `PluginHookBus` + UI timeline。

**Spec:** `docs/superpowers/specs/2026-07-16-hermes-hooks-parity-design.md`

## Global Constraints

- 对齐 Hermes 语义；不加载 Python 插件。
- 不改 Gateway/Shell 体系（同名 shell 旁路继续）。
- 前端仍用 `kind: "hook"`，标题=钩子名。
- `pre_verify`：仅本轮写盘（`terminal` / `file_ops` 变更）且无工具终态时 fire；`KeepGoing` + `MAX_VERIFY_ATTEMPTS=2`。
- transform：首个非 Continue 的 `ReplaceText` 生效（与现网 mutating 短路一致）。
- 插件 panic → warn + Continue。
- 每个 Task 结束可 `cargo test -p hooks` 和/或 `cargo test -p agent`；频繁提交。

## File Structure

| File | Responsibility |
|------|----------------|
| `crates/agent-hooks/src/names.rs` | 7 个常量 + `is_mutating_hook` |
| `crates/agent-hooks/src/outcome.rs` | `ReplaceText` / `KeepGoing` |
| `crates/agent-hooks/src/ui.rs` | `UI_HOOK_NAMES` 加入新名；outcome 字符串映射 |
| `crates/agent-hooks/src/lib.rs` | re-export 新常量 |
| `docs/hooks.md` | 钩子表与顺序 |
| `crates/agent-core/src/runtime/mod.rs` | 写盘标记；`transform_tool_result`；`subagent_stop` 旁补 start 若适用 |
| `crates/agent-core/src/exec/delegate.rs` | `subagent_start` |
| `crates/agent-core/src/streaming/tools_exec.rs` | approval 对；terminal 结果后 `transform_terminal_output` |
| `crates/agent-core/src/streaming/multi_turn.rs` | `pre_verify` + `transform_llm_output` |
| `crates/agent-tools/src/builtin/shell/terminal.rs` | 可选：暴露 raw 或在内 fire（`tools` 已依赖 `hooks`）；优先 agent 包装 |
| `agent/tests/*` / `hooks` 单测 | 覆盖各钩子 |

---

### Task 1: hooks 基础 — 名字 + Outcome + UI

**Files:**
- Modify: `crates/agent-hooks/src/names.rs`
- Modify: `crates/agent-hooks/src/outcome.rs`
- Modify: `crates/agent-hooks/src/ui.rs`
- Modify: `crates/agent-hooks/src/lib.rs`（若需 pub use）
- Modify: `crates/agent-hooks/src/plugin.rs` 测试（可选加 ReplaceText/KeepGoing 短路）

**Interfaces:**
- Produces:
  - `pub const PRE_VERIFY / SUBAGENT_START / PRE_APPROVAL_REQUEST / POST_APPROVAL_RESPONSE / TRANSFORM_TOOL_RESULT / TRANSFORM_TERMINAL_OUTPUT / TRANSFORM_LLM_OUTPUT`
  - `HookOutcome::ReplaceText(String)` / `KeepGoing(String)`
  - `is_mutating_hook` 含上述 mutating 名

- [x] **Step 1: 写失败测试（Outcome 短路）**

在 `crates/agent-hooks/src/plugin.rs` 的 `#[cfg(test)]` 增加：

```rust
#[test]
fn transform_replace_text_short_circuits() {
    let bus = PluginHookBus::new();
    bus.register(TRANSFORM_TOOL_RESULT, |_| HookOutcome::ReplaceText("a".into()));
    bus.register(TRANSFORM_TOOL_RESULT, |_| HookOutcome::ReplaceText("b".into()));
    let out = bus.fire(TRANSFORM_TOOL_RESULT, &HookPayload::default());
    assert!(matches!(out, HookOutcome::ReplaceText(ref s) if s == "a"));
}

#[test]
fn keep_going_short_circuits() {
    let bus = PluginHookBus::new();
    bus.register(PRE_VERIFY, |_| HookOutcome::KeepGoing("retry".into()));
    let out = bus.fire(PRE_VERIFY, &HookPayload::default());
    assert!(matches!(out, HookOutcome::KeepGoing(ref s) if s == "retry"));
}
```

- [x] **Step 2: 跑测确认未实现时失败/编译失败**

```bash
cargo test -p hooks transform_replace_text_short_circuits -- --nocapture
```

Expected: 编译失败（缺变体/常量）或 FAIL

- [x] **Step 3: 实现常量与 Outcome**

`names.rs` 增加 7 常量；`is_mutating_hook`：

```rust
matches!(
    name,
    PRE_LLM_CALL | PRE_TOOL_CALL | PRE_GATEWAY_DISPATCH
        | PRE_VERIFY | TRANSFORM_TOOL_RESULT | TRANSFORM_TERMINAL_OUTPUT | TRANSFORM_LLM_OUTPUT
)
```

`outcome.rs`：

```rust
ReplaceText(String),
KeepGoing(String),
```

`ui.rs`：`UI_HOOK_NAMES` 追加 7 名；若有 `outcome` 格式化函数，为 `ReplaceText`/`KeepGoing` 增加可读字符串（如 `replace_text` / `keep_going`）。

- [x] **Step 4: 跑通 hooks 测试**

```bash
cargo test -p hooks
```

Expected: PASS

- [x] **Step 5: Commit**

```bash
git add hooks docs/hooks.md  # docs 可留 Task 8
git commit -m "$(cat <<'EOF'
feat(hooks): add Hermes parity hook names and outcomes

EOF
)"
```

---

### Task 2: `transform_tool_result`

**Files:**
- Modify: `crates/agent-core/src/runtime/mod.rs`（`handle_tool_call_async` 在 `POST_TOOL_CALL` 前）
- Test: `agent/tests/rig_agent_test.rs` 或新建小测

**Interfaces:**
- Consumes: `TRANSFORM_TOOL_RESULT`、`HookOutcome::ReplaceText`
- Produces: 工具返回字符串可被钩子替换后再记录 / `POST_TOOL_CALL`

- [x] **Step 1: 写失败集成测**

注册 bus：`TRANSFORM_TOOL_RESULT` → `ReplaceText("REDACTED")`；跑一个简单工具；断言结果与 `POST_TOOL_CALL` payload 为 `REDACTED`。

- [x] **Step 2: 跑测见红**

```bash
cargo test -p agent --test rig_agent_test <test_name> -- --nocapture
```

- [x] **Step 3: 实现**

在工具 `dispatch` 返回 `result` 后：

```rust
let result = match self.fire_hook(TRANSFORM_TOOL_RESULT, HookPayload {
    tool_name: Some(name.into()),
    tool_args: Some(args.clone()),
    tool_result: Some(result.clone()),
    session_id: self.session_id.clone(),
    turn_id: self.current_turn_id.clone(),
    ..Default::default()
}) {
    HookOutcome::ReplaceText(s) => s,
    _ => result,
};
// 再 fire POST_TOOL_CALL with final result
```

写盘标记：若 `name == "terminal"` 或 `file_ops` 且 args 表示写入，设 `self.turn_wrote_disk = true`（字段加在 `AgentLoop`，每 `begin_user_turn` 清零）。本 Task 至少落地标记 API，供 Task 7 使用。

- [x] **Step 4: 测试通过 + Commit**

```bash
cargo test -p agent --test rig_agent_test
git commit -m "$(cat <<'EOF'
feat(agent): fire transform_tool_result before post_tool_call

EOF
)"
```

---

### Task 3: `transform_terminal_output`

**Files:**
- Modify: `crates/agent-core/src/streaming/tools_exec.rs` 和/或 `crates/agent-tools/src/builtin/shell/terminal.rs`
- Prefer: 在 agent 拿到 terminal 结果后、若仍接近 raw，则 fire；若 truncation 在 tools 内，则在 `terminal.rs` 截断前 fire（`tools` 已依赖 `hooks`——可通过 `ToolContext` 增加可选 `hook_bus: Option<Arc<PluginHookBus>>`，无 bus 则跳过）

**Interfaces:**
- Consumes: `TRANSFORM_TERMINAL_OUTPUT`
- Produces: 截断前可替换的 raw 输出

- [x] **Step 1: 失败测** — 钩子把 raw 换成短串，断言最终结果不含超长 raw、含替换串

- [x] **Step 2: 实现接线** — 截断前 `fire`；`ReplaceText` 替换后再截断/脱敏

- [x] **Step 3: `cargo test -p agent`（相关）+ `cargo test -p tools`（若动 terminal）+ Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(agent/tools): add transform_terminal_output before truncation

EOF
)"
```

---

### Task 4: `transform_llm_output`

**Files:**
- Modify: `crates/agent-core/src/streaming/multi_turn.rs`（在 `POST_LLM_CALL` 前、最终文本写入/下发前）

**Interfaces:**
- Consumes: `TRANSFORM_LLM_OUTPUT`、`ReplaceText`
- Produces: 最终 assistant 文本可被替换

- [x] **Step 1: 失败测** — recording/bus 断言顺序：`transform_llm_output` 在 `post_llm_call` 前；文本被替换

- [x] **Step 2: 实现**

```rust
let text = match fire(TRANSFORM_LLM_OUTPUT, payload_with_message) {
    ReplaceText(s) => s,
    _ => text,
};
fire(POST_LLM_CALL, ...);
```

- [x] **Step 3: 测试通过 + Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(agent): add transform_llm_output before post_llm_call

EOF
)"
```

---

### Task 5: `pre_approval_request` / `post_approval_response`

**Files:**
- Modify: `crates/agent-core/src/streaming/tools_exec.rs`（Ask / smart / `park_confirm` 路径）

**Interfaces:**
- Consumes: `PRE_APPROVAL_REQUEST`、`POST_APPROVAL_RESPONSE`（观察，忽略返回值除 Continue）
- Payload: `message`=command；`detail` 含 surface/`ask`/`smart`/choice

- [x] **Step 1: 失败测** — recording 捕获审批前后顺序：`pre_approval_request` → … → `post_approval_response`

- [x] **Step 2: 实现**

在分类为 `Ask` 且即将 smart/park 前 fire pre；在 Auto 降级、Deny、或 park 返回后 fire post（`detail` 写明 `auto`/`deny`/`allow`/`timeout` 等）。

- [x] **Step 3: 测试 + Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(agent): fire approval request/response hooks around terminal Ask

EOF
)"
```

---

### Task 6: `subagent_start`

**Files:**
- Modify: `crates/agent-core/src/exec/delegate.rs`（sync/async 入口，每个 child run 前）
- 确认 `runtime` 里仅 `SUBAGENT_STOP` 的路径：start 应在 delegate 内，与 stop 配对

**Interfaces:**
- Consumes: `SUBAGENT_START`
- Produces: 每个 child 在 stop 前必有 start（有 hook bus 时）

- [x] **Step 1: 失败测** — 顺序 `subagent_start` → `subagent_stop`

- [x] **Step 2: 实现** — `run_delegate` 开头 fire start（payload：父 session、任务摘要）；现有 stop 保留

- [x] **Step 3: 测试 + Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(agent): fire subagent_start before delegate child runs

EOF
)"
```

---

### Task 7: `pre_verify` 控制流

**Files:**
- Modify: `crates/agent-core/src/streaming/multi_turn.rs`
- Modify: `crates/agent-core/src/runtime/mod.rs`（`turn_wrote_disk` / `begin_user_turn` 清零；getter）

**Interfaces:**
- Consumes: `PRE_VERIFY`、`KeepGoing`、`MAX_VERIFY_ATTEMPTS = 2`
- Produces: KeepGoing 时注入提示并再进 API 循环

- [x] **Step 1: 失败测**

1. 本轮无写盘 → 不出现 `pre_verify`
2. 写盘 + KeepGoing → 再出现一轮 `pre_api_request`，且最多 2 次 verify 尝试

- [x] **Step 2: 实现**

在「无 tool_calls 的最终 assistant 文本」分支、调用 `transform_llm_output` / `POST_LLM_CALL` 之前：

```rust
if agent.turn_wrote_disk() && verify_attempt < MAX {
    verify_attempt += 1;
    match fire(PRE_VERIFY, ...) {
        KeepGoing(msg) => {
            // 注入用户侧消息或等价 inject，continue 外层 loop
        }
        _ => {}
    }
}
```

- [x] **Step 3: 全量 agent 相关测 + Commit**

```bash
cargo test -p agent
git commit -m "$(cat <<'EOF'
feat(agent): add pre_verify with KeepGoing retry limit

EOF
)"
```

---

### Task 8: 文档与回归

**Files:**
- Modify: `docs/hooks.md`（表 + 顺序图）
- Verify: 全量测试

- [x] **Step 1: 更新 `docs/hooks.md`** — 写入 7 钩子、顺序、`ReplaceText`/`KeepGoing` 说明

- [x] **Step 2: 回归**

```bash
cargo test -p hooks
cargo test -p agent
cargo check -p backend
```

Expected: PASS

- [x] **Step 3: Commit**

```bash
git commit -m "$(cat <<'EOF'
docs(hooks): document Hermes parity hooks

EOF
)"
```

---

## Spec coverage

| Spec | Task |
|------|------|
| 名字 + Outcome + UI | 1 |
| transform_tool_result | 2 |
| transform_terminal_output | 3 |
| transform_llm_output | 4 |
| approval 对 | 5 |
| subagent_start | 6 |
| pre_verify A + attempt | 7 |
| docs + 回归 | 8 |

## Placeholder scan

无 TBD；terminal 接线允许 tools 内 fire 或 agent 包装，以实现时 Cargo/调用链更短者为准。
