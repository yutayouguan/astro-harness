# Hooks Telemetry Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Shell Hook 能读到 `ASTRO_HOOK_TURN`（= turn_id），并提供可配置 URL 的示例 webhook 脚本；默认不引入 OTEL/Langfuse SDK。

**Architecture:** 扩展 `HookPayload.turn_id`；`ShellHookRunner` 有值才写入 `ASTRO_HOOK_TURN`；agent/streaming（及其它 fire 站点）注入 `current_turn_id`；文档示例脚本 `curl` POST JSON。

**Tech Stack:** Rust (`hooks`, `agent`, `backend`)、shell/`curl`、现有 `config.yaml` `hooks:` map

**Spec:** [`docs/superpowers/specs/2026-07-14-hooks-telemetry-export-design.md`](../specs/2026-07-14-hooks-telemetry-export-design.md)

**Naming:** 禁止 `hermes` / `Hermes`。

---

## File map

| Path | Responsibility |
|------|----------------|
| `hooks/src/outcome.rs` | `HookPayload.turn_id` |
| `hooks/src/shell.rs` | `ASTRO_HOOK_TURN`；单元测试 `env_from_payload` |
| `hooks/src/{lib,plugin,ui}.rs` 等 | 字面量补 `turn_id: None` |
| `agent/src/loop_.rs` | fire 时 `turn_id: self.current_turn_id.clone()` |
| `agent/src/streaming.rs` | 同上（取 agent 锁后） |
| `backend/src/grpc/astro_service.rs` | 无 turn 时 `None`；有 session context 尽量贯通 |
| `docs/examples/hooks/telemetry-webhook.sh` | 新建示例脚本 |
| `docs/examples/hooks/config.yaml.snippet` | 补充遥测样例 |
| `docs/examples/hooks/README.md` | 用法 |
| `docs/hooks.md` | 短节「遥测旁路」 |

---

## Task 1: `HookPayload.turn_id` + Shell `ASTRO_HOOK_TURN`（TDD）

**Files:**
- Modify: `hooks/src/outcome.rs`
- Modify: `hooks/src/shell.rs`
- Modify: `hooks/src/lib.rs`、`plugin.rs`、`ui.rs`（测试/字面量）

- [ ] **Step 1: Write failing tests in `shell.rs` `#[cfg(test)]`**

先把 `env_from_payload` 保持 `pub(crate)`（或 `cfg(test)` 可见）以便测：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::outcome::HookPayload;

    #[test]
    fn env_includes_turn_when_set() {
        let env = env_from_payload(
            "post_tool_call",
            &HookPayload {
                session_id: "s1".into(),
                turn_id: Some("turn-abc".into()),
                detail: "d".into(),
                ..Default::default()
            },
        );
        assert!(env.iter().any(|(k, v)| k == "ASTRO_HOOK_TURN" && v == "turn-abc"));
        assert!(env.iter().any(|(k, v)| k == "ASTRO_HOOK_SESSION" && v == "s1"));
    }

    #[test]
    fn env_omits_turn_when_none() {
        let env = env_from_payload(
            "post_tool_call",
            &HookPayload {
                session_id: "s1".into(),
                turn_id: None,
                detail: "d".into(),
                ..Default::default()
            },
        );
        assert!(!env.iter().any(|(k, _)| k == "ASTRO_HOOK_TURN"));
    }
}
```

- [ ] **Step 2: Run — expect FAIL**

```bash
cargo test -p hooks env_includes_turn -- --nocapture
```

Expected: missing field `turn_id` or test fail.

- [ ] **Step 3: Implement**

`outcome.rs`:

```rust
pub struct HookPayload {
    pub session_id: String,
    /// 当前流式回合 id（= agent `current_turn_id` / run_id）；与 `turn`（轮次计数）不同。
    pub turn_id: Option<String>,
    pub detail: String,
    // ... rest unchanged
}
```

`Default` 派生即可（`turn_id: None`）。

`shell.rs` `env_from_payload`：

```rust
if let Some(tid) = payload.turn_id.as_ref().filter(|s| !s.is_empty()) {
    env.push(("ASTRO_HOOK_TURN".into(), tid.clone()));
}
```

Fix all in-crate `HookPayload { ... }` to include `turn_id: None` (or `..Default::default()` where possible).

- [ ] **Step 4: Tests PASS**

```bash
cargo test -p hooks env_includes_turn
cargo test -p hooks env_omits_turn
cargo test -p hooks
```

- [ ] **Step 5: Commit**

```bash
git add hooks/
git commit -m "$(cat <<'EOF'
feat(hooks): add turn_id to HookPayload and ASTRO_HOOK_TURN env

EOF
)"
```

---

## Task 2: Agent / streaming / backend fire 站点贯通

**Files:**
- Modify: `agent/src/loop_.rs`
- Modify: `agent/src/streaming.rs`
- Modify: `backend/src/grpc/astro_service.rs`
- 其它 `HookPayload {` 编译失败处（`rg 'HookPayload \{'`）

- [ ] **Step 1: Compile after Task 1 — list breakages**

```bash
cargo check -p agent -p backend 2>&1 | head -80
```

- [ ] **Step 2: Fill turn_id at fire sites**

规则：

| 位置 | `turn_id` 值 |
|------|----------------|
| `AgentLoop` 方法内 | `self.current_turn_id.clone()` |
| `streaming.rs` 持有 `session: Mutex<AgentLoop>` | 锁后 `agent.current_turn_id().map(str::to_string)` 或 clone `Option` |
| backend gateway 入站 / 无 agent turn | `None` |
| 测试字面量 | `None` 或 `..Default::default()` |

示例（loop_）：

```rust
::hooks::HookPayload {
    session_id: self.session_id.clone(),
    turn_id: self.current_turn_id.clone(),
    detail: "...".into(),
    // ...
}
```

streaming：

```rust
let turn_id = {
    let agent = session.lock().await;
    agent.current_turn_id().map(str::to_string)
};
::hooks::HookPayload {
    session_id: ...,
    turn_id,
    ...
}
```

- [ ] **Step 3: Verify**

```bash
cargo check -p agent -p backend -p hooks
cargo test -p hooks
```

- [ ] **Step 4: Commit**

```bash
git add agent/src/loop_.rs agent/src/streaming.rs backend/src/grpc/astro_service.rs
# plus any other fixed literals
git commit -m "$(cat <<'EOF'
feat(agent): pass current_turn_id into HookPayload

EOF
)"
```

---

## Task 3: 示例脚本 + 文档

**Files:**
- Create: `docs/examples/hooks/telemetry-webhook.sh`
- Modify: `docs/examples/hooks/config.yaml.snippet`
- Modify: `docs/examples/hooks/README.md`
- Modify: `docs/hooks.md`

- [ ] **Step 1: Script**（POSIX sh / bash；`set -eu` 可选；**恒 exit 0**）

```bash
#!/usr/bin/env bash
# Astro Shell Hook telemetry webhook example.
# Copy to ~/.astro/hooks/telemetry-webhook.sh && chmod +x
# Set ASTRO_TELEMETRY_URL (optional ASTRO_TELEMETRY_TOKEN).

set -u
url="${ASTRO_TELEMETRY_URL:-}"
if [[ -z "$url" ]]; then
  exit 0
fi

ts="$(date -u +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || date -u)"
event="${ASTRO_HOOK_EVENT:-}"
session="${ASTRO_HOOK_SESSION:-}"
turn="${ASTRO_HOOK_TURN:-}"
tool="${ASTRO_HOOK_TOOL:-}"
detail="${ASTRO_HOOK_DETAIL:-}"

# Minimal JSON (no jq required)
json=$(printf '{"ts":"%s","event":"%s","session_id":"%s","turn_id":"%s","tool":"%s","detail":"%s"}' \
  "$ts" "$event" "$session" "$turn" "$tool" "$detail")

auth=()
if [[ -n "${ASTRO_TELEMETRY_TOKEN:-}" ]]; then
  auth=(-H "Authorization: Bearer ${ASTRO_TELEMETRY_TOKEN}")
fi

curl -sS -m 4 -X POST "$url" \
  -H "Content-Type: application/json" \
  "${auth[@]}" \
  -d "$json" >/dev/null 2>&1 || true

exit 0
```

注意：`detail`/`tool` 含引号时原始 printf 可能坏 JSON——文档注明「示例级」；可选用简单转义或 `python3 -c`（可选增强，**默认保持最小**）。若担心破坏，detail 可用空或截断且 escape `"`/`\`。

更稳的无 jq 最小转义函数写进脚本注释块；实现时加 3～5 行 escape。

- [ ] **Step 2: config snippet + README**

`config.yaml.snippet` 追加注释块：

```yaml
# 遥测旁路（复制 telemetry-webhook.sh 后启用）
# hooks:
#   post_tool_call: '"$HOME/.astro/hooks/telemetry-webhook.sh"'
#   post_llm_call: '"$HOME/.astro/hooks/telemetry-webhook.sh"'
#   on_session_end: '"$HOME/.astro/hooks/telemetry-webhook.sh"'
```

README 增加「Telemetry webhook」：`ASTRO_TELEMETRY_URL` / TOKEN、chmod、安全（勿上传全文、勿 echo token）。

`docs/hooks.md` 增加小节 **遥测旁路（Shell）**：链到示例；说明 `ASTRO_HOOK_TURN` vs `turn` 轮次字段。

- [ ] **Step 3: Commit**

```bash
git add docs/examples/hooks/ docs/hooks.md
git commit -m "$(cat <<'EOF'
docs(hooks): add telemetry webhook shell example

EOF
)"
```

---

## Task 4: 验收 + 标记 spec

- [ ] **Step 1: Regressions**

```bash
cargo test -p hooks
cargo check -p agent -p backend
```

- [ ] **Step 2: Spec status**

`docs/superpowers/specs/2026-07-14-hooks-telemetry-export-design.md`：`已批准（待实现）` → `已批准 / 已实现`  
计划 Tasks 勾选。

- [ ] **Step 3: Commit**

```bash
git commit -m "$(cat <<'EOF'
docs: mark hooks telemetry export implemented

EOF
)"
```

---

## Spec coverage self-check

| Spec | Task |
|------|------|
| HookPayload.turn_id + ASTRO_HOOK_TURN（有值才设） | Task 1 |
| Agent/streaming fire 贯通 | Task 2 |
| telemetry-webhook.sh + 文档 | Task 3 |
| 无默认 OTEL 依赖 | 全计划遵守 |
| 不传 tool_args 到 env | Task 1 不改其它 env |

**Placeholder scan:** 无 TBD。  
**Type consistency:** `turn_id: Option<String>` / `ASTRO_HOOK_TURN` / `current_turn_id`。
