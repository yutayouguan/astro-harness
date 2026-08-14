# Chat Fallback Chain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Primary 在首包前因 429/5xx/401·403/网络失败时，按 `providers.json` 显式 `fallback` 链临时切换模型；覆盖主聊、cron、delegate，且不改 `active_provider_id`。

**Architecture:** `common::ChatTarget` + 纯函数 `expand_chat_targets`；Tauri 读 `providers.json`/keyring 展开链后经 gRPC `ChatRequest.chat_fallbacks` 下传（对齐图片 fallback）；`agent` 内 `try_stream_completion_with_fallback` 包装每次 LLM 调用；cron/delegate 携带同一 `Vec`。

**Tech Stack:** Rust、tonic/proto、Tauri、serde_json、既有 `ProviderRegistry` / `ProviderStreamer`、tempfile 单测

**Spec:** [`docs/superpowers/specs/2026-07-14-chat-fallback-chain-design.md`](../specs/2026-07-14-chat-fallback-chain-design.md)

**Status:** Tasks 1–8 completed on branch `feat/chat-fallback-chain`.

**执行注意:** 在干净 worktree 实现；用户可见文案与代码标识勿写上游品牌字样；`fallback` 最多 3 条。

---

## File Structure

| File | Responsibility |
|------|----------------|
| Create: `common/src/chat_target.rs` | `ChatTarget`、`FallbackRef`、`expand_chat_targets`、`MAX_CHAT_FALLBACKS=3` |
| Modify: `common/src/lib.rs` | 导出 chat_target |
| Create: `crates/agent-core/src/chat_fallback.rs` | `is_failover_eligible`、`try_stream_completion_with_fallback`、`ActiveTargetMeta` |
| Modify: `crates/agent-core/src/lib.rs` | `mod chat_fallback` + 导出 |
| Modify: `crates/agent-core/src/streaming.rs` | `ProviderStreamer` / `run_multi_turn_stream*` 吃 `Vec<ChatTarget>`；usage 用实际 hit |
| Modify: `crates/agent-core/src/cron_exec.rs` | `CronExecCredentials.targets`；`chat_stream` → helper |
| Modify: `crates/agent-core/src/delegate_exec.rs` | 使用 `req.chat_targets` |
| Modify: `crates/agent-core/src/loop_.rs` | 可选缓存 `chat_targets` 注入 ToolContext |
| Modify: `crates/agent-memory/src/delegate_spawn.rs` | `DelegateRunRequest.chat_targets: Vec<common::ChatTarget>` |
| Modify: `crates/agent-tools/src/engine/context.rs` | `ToolContext.chat_targets` |
| Modify: `crates/agent-tools/src/builtin/delegate.rs` | `build_run_request` 下传链 |
| Modify: `proto/proto/astro.proto` | `ChatFallbackTarget` + `repeated chat_fallbacks = 20` |
| Modify: `crates/agent-server/src/grpc/astro_service.rs` | 把 fallbacks 组成 `Vec<ChatTarget>` 传入 multi_turn |
| Modify: `apps/desktop/src-tauri/src/providers_commands.rs` | `ProviderConfig.fallback`、`resolve_chat_targets`、DTO |
| Modify: `apps/desktop/src-tauri/src/commands.rs` | `start_chat` / cron resolve 填链 |
| Modify: `apps/desktop/src/components/ProvidersPanel.tsx` | MVP 后备编辑 UI |
| Modify: `docs/superpowers/specs/2026-07-14-chat-fallback-chain-design.md` | 实现后状态 → 已实现 |

---

### Task 1: `common::ChatTarget` + `expand_chat_targets`

**Files:**
- Create: `common/src/chat_target.rs`
- Modify: `common/src/lib.rs`
- Test: `common/src/chat_target.rs` `#[cfg(test)]`

- [x] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, backend: &str, model: &str) -> ChatTarget {
        ChatTarget {
            provider_id: id.into(),
            backend_id: backend.into(),
            model: model.into(),
            api_key: format!("k-{id}"),
            base_url: format!("https://{id}.example"),
        }
    }

    #[test]
    fn expand_skips_missing_dedups_and_caps() {
        let primary = t("p0", "openai", "gpt");
        let catalog = vec![
            t("p0", "openai", "gpt"),
            t("p1", "claude", "opus"),
            t("p2", "deepseek", "chat"),
            t("p3", "zhipu", "glm"),
            t("p4", "ollama", "llama"), // 第 4 个后备应被截断
        ];
        let refs = vec![
            FallbackRef { provider_id: "p1".into(), model: Some("opus-x".into()) },
            FallbackRef { provider_id: "missing".into(), model: None },
            FallbackRef { provider_id: "p1".into(), model: None }, // dup
            FallbackRef { provider_id: "p0".into(), model: None }, // self
            FallbackRef { provider_id: "p2".into(), model: None },
            FallbackRef { provider_id: "p3".into(), model: None },
            FallbackRef { provider_id: "p4".into(), model: None },
        ];
        let lookup = |id: &str| catalog.iter().find(|c| c.provider_id == id).cloned();
        let chain = expand_chat_targets(&primary, &refs, lookup);
        assert_eq!(chain.len(), 1 + MAX_CHAT_FALLBACKS); // primary + 3
        assert_eq!(chain[0].provider_id, "p0");
        assert_eq!(chain[1].model, "opus-x");
        assert_eq!(chain[2].provider_id, "p2");
        assert_eq!(chain[3].provider_id, "p3");
    }
}
```

- [x] **Step 2: Run — expect FAIL**

```bash
cargo test -p common expand_skips_missing -- --nocapture
```

- [x] **Step 3: 实现**

`common/src/chat_target.rs`：

```rust
use serde::{Deserialize, Serialize};

pub const MAX_CHAT_FALLBACKS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatTarget {
    pub provider_id: String,
    pub backend_id: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackRef {
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// `lookup` 返回已解析凭据的条目（含 key）；None 则跳过。
pub fn expand_chat_targets<F>(
    primary: &ChatTarget,
    fallbacks: &[FallbackRef],
    mut lookup: F,
) -> Vec<ChatTarget>
where
    F: FnMut(&str) -> Option<ChatTarget>,
{
    let mut out = vec![primary.clone()];
    let mut seen = std::collections::HashSet::new();
    seen.insert(primary.provider_id.clone());
    for fr in fallbacks.iter().take(MAX_CHAT_FALLBACKS * 2) {
        // 多读一点以便跳过后仍能填满 3 条
        if out.len() >= 1 + MAX_CHAT_FALLBACKS {
            break;
        }
        if fr.provider_id.is_empty() || !seen.insert(fr.provider_id.clone()) {
            continue;
        }
        let Some(mut t) = lookup(&fr.provider_id) else { continue };
        if let Some(m) = fr.model.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            t.model = m.to_string();
        }
        // key 空且非本地无需 key 的 backend：仍允许 ollama（api_key 可空）
        let allow_empty_key = t.backend_id == "ollama";
        if t.api_key.trim().is_empty() && !allow_empty_key {
            continue;
        }
        out.push(t);
    }
    out
}
```

`lib.rs` 增加 `pub mod chat_target; pub use chat_target::*;`

- [x] **Step 4: tests PASS**

```bash
cargo test -p common -- --nocapture
```

- [x] **Step 5: Commit**

```bash
git add common/src/chat_target.rs common/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(common): add ChatTarget and expand_chat_targets for fallback chains

EOF
)"
```

---

### Task 2: `is_failover_eligible` + stream helper

**Files:**
- Create: `crates/agent-core/src/chat_fallback.rs`
- Modify: `crates/agent-core/src/lib.rs`
- Test: `crates/agent-core/src/chat_fallback.rs` `#[cfg(test)]`

- [x] **Step 1: 写失败测试（错误分类）**

```rust
#[test]
fn failover_eligible_for_429_and_5xx() {
    assert!(is_failover_eligible(&anyhow::anyhow!("HTTP 429 Too Many Requests")));
    assert!(is_failover_eligible(&anyhow::anyhow!("status 503 service unavailable")));
    assert!(is_failover_eligible(&anyhow::anyhow!("401 Unauthorized")));
    assert!(is_failover_eligible(&anyhow::anyhow!("connection reset by peer")));
    assert!(!is_failover_eligible(&anyhow::anyhow!("HTTP 400 bad request")));
    assert!(!is_failover_eligible(&anyhow::anyhow!("cancelled by user")));
}
```

- [x] **Step 2: Run — FAIL**

```bash
cargo test -p agent failover_eligible_for_429 -- --nocapture
```

- [x] **Step 3: 实现分类 + helper 骨架**

`is_failover_eligible`：对 `err.to_string().to_ascii_lowercase()` 匹配 `429`、`rate limit`、`401`、`403`、`unauthorized`、`forbidden`、`500`–`599` / `502`/`503`/`504`、`timeout`、`connection`、`tls`、`dns`；排除 `400`、`cancelled`/`canceled`/`aborted`（若同时含 429 以可切为准：先匹配可切关键字再排除取消）。

`ActiveTargetMeta { provider_id, backend_id, model, base_url }`

`try_stream_completion_with_fallback` 逻辑（伪代码，实现时补全类型）：

```rust
pub async fn try_stream_completion_with_fallback(
    targets: &[ChatTarget],
    registry: &ProviderRegistry,
    messages: Vec<ProviderMessage>,
    tools: Vec<serde_json::Value>,
    base_config: &ProviderConfig, // temperature 等从这里拷，覆盖 model/key/url
    mut on_failover: impl FnMut(&ChatTarget, &ChatTarget, &anyhow::Error),
) -> anyhow::Result<(ChatStream, ActiveTargetMeta)> {
    let mut errors = Vec::new();
    for (i, target) in targets.iter().enumerate() {
        let provider = registry
            .get(&target.backend_id)
            .ok_or_else(|| anyhow::anyhow!("未知 Provider: {}", target.backend_id))?;
        let config = ProviderConfig {
            api_key: target.api_key.clone(),
            base_url: Some(target.base_url.clone()).filter(|s| !s.is_empty()),
            model: target.model.clone(),
            temperature: base_config.temperature,
            max_tokens: base_config.max_tokens,
            thinking_enabled: base_config.thinking_enabled,
            reasoning_effort: base_config.reasoning_effort.clone(),
            additional_params: base_config.additional_params.clone(),
        };
        match provider.chat_stream(messages.clone(), tools.clone(), &config).await {
            Ok(stream) => {
                // 包一层：在首包前错误则 Err；见 Step 3b
                match probe_or_wrap_pre_content(stream).await {
                    Ok(s) => {
                        return Ok((s, ActiveTargetMeta { /* from target */ }));
                    }
                    Err(e) if is_failover_eligible(&e) && i + 1 < targets.len() => {
                        on_failover(target, &targets[i + 1], &e);
                        errors.push(format!("{}: {e}", target.backend_id));
                    }
                    Err(e) => {
                        errors.push(format!("{}: {e}", target.backend_id));
                        break;
                    }
                }
            }
            Err(e) if is_failover_eligible(&e) && i + 1 < targets.len() => {
                on_failover(target, &targets[i + 1], &e);
                errors.push(format!("{}: {e}", target.backend_id));
            }
            Err(e) => {
                errors.push(format!("{}: {e}", target.backend_id));
                break;
            }
        }
    }
    anyhow::bail!("全部模型尝试失败：{}", errors.join("；"))
}
```

**首包前包装（`probe_or_wrap_pre_content`）建议实现：**

- 用 `futures::StreamExt` 缓冲：peek 首个 `Result<ChatChunk>`  
- 若 `Err` 或 chunk 仅有 `finish_reason` 且以 `error:` 开头且无文本/tool/reasoning → 返回该 Err  
- 否则构造新流：先 yield 已 peek 项，再转发剩余；标记 `content_started`  

注意：一旦 `Ok` 返回给上层并开始 `map_provider_stream` 消费，中途再失败 **不可** 在 helper 外再切（由「已返回 Ok(stream)」保证；若 peek 消耗了一项，勿丢）。

- [x] **Step 4: 单测 + `cargo test -p agent chat_fallback` PASS**

额外测：用假 stream 模拟「首事件 error:」可切；「已有 delta 后 Err」在包装层之外测「已锁定」可通过对 `try_*` mock registry 完成（若无 mock 基础设施，至少保证分类测 + 单元测 peek 辅助函数）。

- [x] **Step 5: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(agent): add chat failover eligibility and stream fallback helper

EOF
)"
```

---

### Task 3: 接线 `ProviderStreamer` / multi_turn

**Files:**
- Modify: `crates/agent-core/src/streaming.rs`
- Modify: 所有 `run_multi_turn_stream` / `stream_multi_turn_with_hitl` 调用方签名

- [x] **Step 1: 扩展签名**

将 `run_multi_turn_stream(..., provider, config, ...)` 改为接收：

```rust
targets: Vec<common::ChatTarget>,
registry: Arc<ProviderRegistry>, // 或保留 Arc<dyn AiProvider> 仅作 primary 兼容
base_config: ProviderConfig,     // temperature 等
```

MVP 简化：保留 `provider: Arc<dyn AiProvider>` 不用，改为每跳 `registry.get(backend_id)`。若改动面过大，则 `ProviderStreamer` 持有：

```rust
pub struct ProviderStreamer {
    pub registry: Arc<ProviderRegistry>,
    pub targets: Vec<ChatTarget>,
    pub base_config: ProviderConfig,
}
```

`stream_completion` 调 `try_stream_completion_with_fallback`，再用现有 `map_provider_stream`。

成功后把 `ActiveTargetMeta` 写进某 `Cell`/`Mutex` 供本轮 `record_llm_usage` 取真实 provider/model/base_url（替换原先仅从 `AgentLoop` 读 primary 凭据）。

- [x] **Step 2: failover 时可选 `emit(Status(...))`**

在 `on_failover` 闭包里若有 `tx`：发 `MultiTurnStreamItem` 中已有的状态类变体（若无则 `tracing::warn!` 即可，不新增 proto）。

- [x] **Step 3: 编译 + 相关测试**

```bash
cargo test -p agent -- --nocapture
cargo check -p backend
```

- [x] **Step 4: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(agent): wire ProviderStreamer and multi_turn to chat fallback targets

EOF
)"
```

---

### Task 4: Proto + backend 下传

**Files:**
- Modify: `proto/proto/astro.proto`
- Rebuild proto（仓库既有脚本 / `cargo build -p proto`）
- Modify: `crates/agent-server/src/grpc/astro_service.rs`

- [x] **Step 1: Proto 增量**

```protobuf
message ChatFallbackTarget {
  string provider = 1;   // backend_id（registry）
  string model = 2;
  string api_key = 3;
  string base_url = 4;
  string provider_id = 5; // providers.json 条目 id，可空
}

message ChatRequest {
  // ... existing fields ...
  repeated ChatFallbackTarget chat_fallbacks = 20;
}
```

`chat_fallbacks` = **不含 primary** 的后备列表（primary 仍用字段 3/4/7/8）；backend 组装：

```rust
let mut targets = vec![ChatTarget {
    provider_id: String::new(), // 或日后扩展
    backend_id: provider_name.clone(),
    model: model.clone(),
    api_key: api_key.clone(),
    base_url: base_url.clone(),
}];
for fb in req.chat_fallbacks {
    targets.push(ChatTarget {
        provider_id: fb.provider_id,
        backend_id: fb.provider,
        model: fb.model,
        api_key: fb.api_key,
        base_url: fb.base_url,
    });
}
```

传入 `stream_multi_turn_with_hitl`。

- [x] **Step 2: build + check**

```bash
cargo build -p proto -p backend
```

- [x] **Step 3: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(proto): add ChatFallbackTarget list on ChatRequest

EOF
)"
```

---

### Task 5: Tauri `providers.json` + `resolve_chat_targets`

**Files:**
- Modify: `apps/desktop/src-tauri/src/providers_commands.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`（`start_chat` / `run_chat_stream` / `resolve_creds_for_job`）

- [x] **Step 1: 数据结构**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderFallbackEntry {
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

// ProviderConfig / Input / Dto 增加:
#[serde(default)]
pub fallback: Vec<ProviderFallbackEntry>,
```

- [x] **Step 2: `resolve_chat_targets(primary_provider_id, model_override)`**

1. `find_provider` primary → key/endpoint/backend  
2. 构造 primary `ChatTarget`（model：参数覆盖或条目 model）  
3. `expand_chat_targets` + lookup：`find_provider` + `resolve_api_key`；disabled 跳过  
4. 返回 `Vec<ChatTarget>`（含 primary）

单测：可用 tempfile 写临时 providers 状态测 expand 调用（若命令难测，至少测 serialize 兼容无 `fallback` 字段）。

- [x] **Step 3: `start_chat`**

```rust
let targets = resolve_chat_targets(provider_id.as_deref(), &provider, &model)?;
// primary 仍填 ChatRequest 字段；fallbacks = targets[1..] 映射为 ChatFallbackTarget
```

Cron：`CronExecCredentials` 增加 `targets: Vec<ChatTarget>`（或保留四字段为 primary 并由 targets 覆盖）。

- [x] **Step 4:**

```bash
cargo check -p astro-ui --manifest-path apps/desktop/src-tauri/Cargo.toml
# 包名以实际 Cargo.toml name 为准
```

- [x] **Step 5: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(tauri): resolve chat fallback chain from providers.json

EOF
)"
```

---

### Task 6: Cron + Delegate 下传

**Files:**
- Modify: `crates/agent-core/src/cron_exec.rs`、`agent/tests/cron_exec_test.rs`
- Modify: `crates/agent-memory/src/delegate_spawn.rs`
- Modify: `crates/agent-tools/src/engine/context.rs`、`crates/agent-tools/src/builtin/delegate.rs`
- Modify: `crates/agent-core/src/loop_.rs`（构造 ToolContext 时填 `chat_targets`）
- Modify: `crates/agent-core/src/delegate_exec.rs`

- [x] **Step 1: `CronExecCredentials`**

```rust
pub struct CronExecCredentials {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    pub targets: Vec<common::ChatTarget>,
}
```

空 `targets` 时 fallback 为单元素 `vec![ChatTarget { …四字段… }]`。所有 `chat_stream` 改为 helper。

- [x] **Step 2: Delegate**

```rust
// DelegateRunRequest
pub chat_targets: Vec<common::ChatTarget>, // default empty
```

`req_clone_creds` 拷贝之；子 Agent `chat_stream` 用 helper。`ToolContext` 增加 `chat_targets: &'a [ChatTarget]`（或 owned Vec）；`build_run_request` 填入。`AgentLoop` 存 `chat_targets: Vec<ChatTarget>`，`set_chat_credentials` 旁增加 `set_chat_targets`，backend 开聊时设置。

- [x] **Step 3: 修编译与测试**

```bash
cargo test -p agent -p tools -p memory -- --nocapture
```

- [x] **Step 4: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(agent): propagate chat fallback targets through cron and delegate

EOF
)"
```

---

### Task 7: Providers 面板 MVP UI

**Files:**
- Modify: `apps/desktop/src/components/ProvidersPanel.tsx`
- 类型定义处（若有独立 `types`）同步 `fallback?: { provider_id: string; model?: string }[]`

- [x] **Step 1: draft 状态增加 `fallback`**

- 下拉：其它 `enabled` 且 `id !== selected.id` 的 providers  
- 「添加后备」最多 3；可选 model 文本框  
- 删除 / 上移下移（可选：仅删除+添加）  
- `save_provider` / `toggleEnabled` payload 带上 `fallback`

- [x] **Step 2: 手动或 `npm run build`（frontend）确认类型通过**

- [x] **Step 3: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(ui): edit chat fallback chain on provider settings

EOF
)"
```

---

### Task 8: 文档收尾

**Files:**
- Modify: `docs/superpowers/specs/2026-07-14-chat-fallback-chain-design.md` 状态 → **已实现**
- 可选：`README` 或 providers 帮助一句

- [x] **Step 1: 更新状态与交叉引用**

- [x] **Step 2: Commit**

```bash
git commit -m "$(cat <<'EOF'
docs: mark chat fallback chain design as implemented

EOF
)"
```

---

## Done when

验收见 spec「验收标准」。空 fallback 行为与现网一致；Primary 429 + 后备可用可完整回复且活跃 provider 不变；流中途失败不换模；cron/delegate 在有链时一致。
