# Context Usage Explorer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在每轮请求组装时按 8 类估算上下文占用，经聊天事件推到前端；Composer 浮层 + 右栏 Context Explorer（环形图 + 列表）共用快照，下挂原有活动时间线。

**Architecture:** 新增 `agent::context_usage` 纯函数模块（`ceil(chars/4)` + 归类）；`run_multi_turn_stream` 在每次 `stream_chat` 前组装并 emit `MultiTurnStreamItem::ContextUsage`；经 proto → Tauri → `App` state；共享 `ContextUsageBar` / `ContextExplorer` / `ContextUsagePopover`，主题色走 CSS 变量。

**Tech Stack:** Rust（agent / proto / backend / Tauri）、React + CSS（现有 chat 玻璃风）、前端 `node --experimental-strip-types --test`

**Spec:** [`docs/superpowers/specs/2026-07-15-context-usage-explorer-design.md`](../specs/2026-07-15-context-usage-explorer-design.md)

**执行注意:**
- 勿改账单 `Usage` / `usage_record` 语义；分层快照是独立事件。
- `context_window`：快照字段可带后端值；前端以当前选中模型 `context_window` 覆盖，缺省 `128_000`。
- 分项 tokens 为 0 时 UI 不渲染该行。

---

## File Structure

| File | Responsibility |
|------|----------------|
| Create: `agent/src/context_usage.rs` | 估算、8 类归类、`ContextUsageSnapshot` |
| Modify: `agent/src/lib.rs` | `mod context_usage` + re-export |
| Modify: `agent/src/loop_.rs` | 导出分层字符原料（与 `build_system_prompt` 同源） |
| Modify: `agent/src/streaming.rs` | 组装前 emit `ContextUsage` |
| Modify: `proto/proto/astro.proto` | `ContextUsageEvent` + `ChatEvent` oneof |
| Modify: `backend/src/grpc/astro_service.rs` | `multi_turn_to_chat_event` 映射 |
| Modify: `frontend/src-tauri/src/commands.rs` | `ChatStreamEvent::ContextUsage` + emit |
| Create: `apps/desktop/src/lib/contextUsage.ts` | 类型、格式化、%、过滤 0 |
| Create: `apps/desktop/src/lib/contextUsage.test.ts` | 纯函数测试 |
| Create: `apps/desktop/src/components/ContextUsageBar.tsx` | 分段条 |
| Create: `apps/desktop/src/components/ContextUsagePopover.tsx` | Composer 浮层 |
| Create: `apps/desktop/src/components/ContextExplorer.tsx` | 右栏用量区 |
| Modify: `apps/desktop/src/components/ChatRightPanel.tsx` | context tab：Explorer + Timeline |
| Modify: `apps/desktop/src/components/ChatView.tsx` | % 按钮 ↔ popover；查看详情回调 |
| Modify: `apps/desktop/src/components/ChatAgentInfo.tsx` | 弱化用量，链到上下文 tab |
| Modify: `apps/desktop/src/App.tsx` | 监听事件、state、传 props、模型窗口 |
| Modify: `apps/desktop/src/styles/chat.css` / `chat-right-panel.css` | 浮层 / Explorer / 环图样式 |
| Modify: `apps/desktop/src/i18n/messages.ts` | 文案 |
| Modify: spec | 状态 → 已实现（收尾） |

---

### Task 1: `context_usage` 纯模块（TDD）

**Files:**
- Create: `agent/src/context_usage.rs`
- Modify: `agent/src/lib.rs`

- [ ] **Step 1: 写失败测试（模块内 `#[cfg(test)]`）**

先在 `agent/src/context_usage.rs` 写最小类型与空 `build_snapshot`，再写测试；或先写测试文件后实现——按下面完整模块落盘。

目标测试（实现后放在同文件 `mod tests`）：

```rust
#[test]
fn estimate_tokens_ceil_div_4() {
    assert_eq!(estimate_tokens(0), 0);
    assert_eq!(estimate_tokens(1), 1);
    assert_eq!(estimate_tokens(4), 1);
    assert_eq!(estimate_tokens(5), 2);
}

#[test]
fn mcp_prefix_goes_to_mcp_segment() {
    let tools = serde_json::json!([
        {"type":"function","function":{"name":"file_ops","parameters":{}}},
        {"type":"function","function":{"name":"mcp__fs__read","parameters":{"a":1}}}
    ]);
    let snap = build_snapshot(ContextUsageInput {
        system_chars: 40,
        memory_chars: 0,
        skills_chars: 0,
        recall_chars: 0,
        tools_json: &tools,
        messages: &[],
        context_window: 128_000,
        updated_at_ms: 1,
    });
    let tools_seg = snap.segment("tools").unwrap();
    let mcp_seg = snap.segment("mcp").unwrap();
    assert!(tools_seg.tokens > 0);
    assert!(mcp_seg.tokens > 0);
    assert_eq!(mcp_seg.meta.as_ref().and_then(|m| m.count), Some(1));
}

#[test]
fn delegate_tool_result_counts_as_subagent() {
    use common::message::{Message, ToolCall};
    let assistant = Message::assistant_with_tools(
        "",
        vec![ToolCall {
            id: "c1".into(),
            name: "delegate".into(),
            arguments: serde_json::json!({}),
        }],
    );
    let tool = Message::tool_with_id("c1", &"x".repeat(40));
    let snap = build_snapshot(ContextUsageInput {
        system_chars: 0,
        memory_chars: 0,
        skills_chars: 0,
        recall_chars: 0,
        tools_json: &serde_json::json!([]),
        messages: &[assistant, tool],
        context_window: 128_000,
        updated_at_ms: 1,
    });
    assert_eq!(snap.segment("subagent").map(|s| s.tokens), Some(10));
    assert_eq!(snap.segment("conversation").map(|s| s.tokens).unwrap_or(0), 0);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p agent estimate_tokens_ceil_div_4 -- --nocapture`  
Expected: compile fail 或 test not found（尚未实现）

- [ ] **Step 3: 实现 `agent/src/context_usage.rs`**

```rust
//! 上下文占用分层估算（ceil(chars/4)），与账单 Usage 无关。

use common::message::{Message, Role};
use serde::{Deserialize, Serialize};

pub const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;

const SUBAGENT_TOOLS: &[&str] = &[
    "delegate",
    "delegate_async",
    "async_delegate_collect",
    "orchestration_run",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextUsageSegmentMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextUsageSegment {
    pub id: String,
    pub tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ContextUsageSegmentMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextUsageSnapshot {
    pub context_window: u32,
    pub total_tokens: u32,
    pub segments: Vec<ContextUsageSegment>,
    pub updated_at: i64,
}

impl ContextUsageSnapshot {
    pub fn segment(&self, id: &str) -> Option<&ContextUsageSegment> {
        self.segments.iter().find(|s| s.id == id)
    }
}

pub struct ContextUsageInput<'a> {
    pub system_chars: usize,
    pub memory_chars: usize,
    pub skills_chars: usize,
    pub recall_chars: usize,
    pub tools_json: &'a serde_json::Value,
    pub messages: &'a [Message],
    pub context_window: u32,
    pub updated_at_ms: i64,
}

pub fn estimate_tokens(chars: usize) -> u32 {
    u32::try_from(chars.div_ceil(4)).unwrap_or(u32::MAX)
}

pub fn is_subagent_tool_name(name: &str) -> bool {
    SUBAGENT_TOOLS.contains(&name)
}

fn push_seg(out: &mut Vec<ContextUsageSegment>, id: &str, chars: usize, count: Option<u32>) {
    let tokens = estimate_tokens(chars);
    if tokens == 0 {
        return;
    }
    out.push(ContextUsageSegment {
        id: id.to_string(),
        tokens,
        meta: count.map(|c| ContextUsageSegmentMeta { count: Some(c) }),
    });
}

fn tool_schema_name(tool: &serde_json::Value) -> Option<&str> {
    tool.get("function")
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())
        .or_else(|| tool.get("name").and_then(|n| n.as_str()))
}

pub fn build_snapshot(input: ContextUsageInput<'_>) -> ContextUsageSnapshot {
    let mut tools_chars = 0usize;
    let mut mcp_chars = 0usize;
    let mut tools_n = 0u32;
    let mut mcp_n = 0u32;
    if let Some(arr) = input.tools_json.as_array() {
        for t in arr {
            let s = t.to_string();
            let n = tool_schema_name(t).unwrap_or("");
            if n.starts_with("mcp__") {
                mcp_chars += s.len();
                mcp_n += 1;
            } else {
                tools_chars += s.len();
                tools_n += 1;
            }
        }
    }

    let mut call_names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for m in input.messages {
        if let Some(calls) = &m.tool_calls {
            for c in calls {
                call_names.insert(c.id.clone(), c.name.clone());
            }
        }
    }

    let mut conversation_chars = 0usize;
    let mut subagent_chars = 0usize;
    let mut subagent_n = 0u32;
    let mut msg_n = 0u32;
    for m in input.messages {
        let text = m.content_str();
        match m.role {
            Role::Tool => {
                let name = m
                    .tool_call_id
                    .as_ref()
                    .and_then(|id| call_names.get(id))
                    .map(String::as_str)
                    .unwrap_or("");
                if is_subagent_tool_name(name) {
                    subagent_chars += text.len();
                    subagent_n += 1;
                } else {
                    conversation_chars += text.len();
                    msg_n += 1;
                }
            }
            Role::System => {
                // system 已在分层字符中统计；会话里偶发 system 归入 conversation
                conversation_chars += text.len();
                msg_n += 1;
            }
            _ => {
                conversation_chars += text.len();
                if let Some(calls) = &m.tool_calls {
                    for c in calls {
                        conversation_chars += c.name.len() + c.arguments.to_string().len();
                    }
                }
                msg_n += 1;
            }
        }
    }

    let mut segments = Vec::new();
    push_seg(&mut segments, "system", input.system_chars, None);
    push_seg(&mut segments, "tools", tools_chars, (tools_n > 0).then_some(tools_n));
    push_seg(&mut segments, "mcp", mcp_chars, (mcp_n > 0).then_some(mcp_n));
    push_seg(&mut segments, "memory", input.memory_chars, None);
    push_seg(&mut segments, "skills", input.skills_chars, None);
    push_seg(&mut segments, "recall", input.recall_chars, None);
    push_seg(
        &mut segments,
        "subagent",
        subagent_chars,
        (subagent_n > 0).then_some(subagent_n),
    );
    push_seg(
        &mut segments,
        "conversation",
        conversation_chars,
        (msg_n > 0).then_some(msg_n),
    );

    let total_tokens = segments.iter().map(|s| s.tokens).sum();
    let window = if input.context_window == 0 {
        DEFAULT_CONTEXT_WINDOW
    } else {
        input.context_window
    };

    ContextUsageSnapshot {
        context_window: window,
        total_tokens,
        segments,
        updated_at: input.updated_at_ms,
    }
}
```

在 `agent/src/lib.rs` 加入：

```rust
pub mod context_usage;
pub use context_usage::{ContextUsageSnapshot, ContextUsageSegment, build_snapshot};
```

把 Step 1 的三个 `#[test]` 放进同文件 `#[cfg(test)] mod tests { ... }`。

- [ ] **Step 4: 跑测试通过**

Run: `cargo test -p agent context_usage -- --nocapture`  
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add agent/src/context_usage.rs agent/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(agent): add context usage snapshot estimation

EOF
)"
```

---

### Task 2: 从 AgentLoop 导出分层字符

**Files:**
- Modify: `agent/src/loop_.rs`

- [ ] **Step 1: 新增 `system_prompt_layer_chars`（与 `build_system_prompt` 同源）**

在 `AgentLoop` impl 中，紧挨 `build_system_prompt`，抽取共用装载逻辑或直接复制装载后分别计量：

```rust
/// 与 `build_system_prompt` 同源的分层字符数，供上下文占用估算。
pub fn system_prompt_layer_chars(&self) -> (usize, usize, usize, usize) {
    let (project_memory, user_profile, daily) = self.memory.prompt_snapshot_with_daily();
    let skill_pairs = if self.tool_registry.is_toolset_enabled("skills") {
        skills::list_enabled_for_prompt()
    } else {
        Vec::new()
    };
    let skill_index: Vec<(&str, &str)> = skill_pairs
        .iter()
        .map(|(name, desc)| (name.as_str(), desc.as_str()))
        .collect();

    let static_ctx = if let Some(ref over) = self.config.static_override {
        over.clone()
    } else {
        crate::context::StaticContext::from_workspace_files(
            &self.config.soul,
            &project_memory,
            &user_profile,
            &daily,
        )
    };
    let dynamic_ctx = crate::context::DynamicContext::from_recalled(
        self.config.dynamic_max_items,
        &self.last_recalled_context,
    );

    // system：soul / identity / agent_md + guidance + timestamp（不含 memory 三件套）
    let mut system = String::new();
    if !static_ctx.soul.trim().is_empty() {
        system.push_str(static_ctx.soul.trim());
    }
    if !static_ctx.identity.trim().is_empty() {
        system.push_str(static_ctx.identity.trim());
    }
    if !static_ctx.agent_md.trim().is_empty() {
        system.push_str(static_ctx.agent_md.trim());
    }
    system.push_str("工具使用"); // guidance + timestamp 用真实渲染更准：
    let guidance = crate::prompt_builder::PromptBuilder::new()
        .with_tool_guidance()
        .with_timestamp()
        .build();
    system.push_str(&guidance);

    let memory = format!(
        "{}{}{}",
        static_ctx.memory.trim(),
        static_ctx.user_profile.trim(),
        static_ctx.daily.trim()
    );
    let skills = crate::prompt_builder::PromptBuilder::new()
        .with_skills_index(&skill_index)
        .build();
    let recall = dynamic_ctx.render();

    (system.len(), memory.len(), skills.len(), recall.len())
}
```

**注意：** 上面 guidance 示意需改成只调用现有 builder，避免字面量漂移。推荐最终写法：

```rust
let guidance_ts = PromptBuilder::new().with_tool_guidance().with_timestamp().build();
let mut system_chars = guidance_ts.len();
for part in [&static_ctx.soul, &static_ctx.identity, &static_ctx.agent_md] {
    system_chars += part.trim().len();
}
let memory_chars = static_ctx.memory.trim().len()
    + static_ctx.user_profile.trim().len()
    + static_ctx.daily.trim().len();
let skills_chars = PromptBuilder::new().with_skills_index(&skill_index).build().len();
let recall_chars = dynamic_ctx.render().len();
(system_chars, memory_chars, skills_chars, recall_chars)
```

- [ ] **Step 2: 编译**

Run: `cargo check -p agent`  
Expected: OK

- [ ] **Step 3: Commit**

```bash
git add agent/src/loop_.rs
git commit -m "$(cat <<'EOF'
feat(agent): expose prompt layer char counts for usage

EOF
)"
```

---

### Task 3: 流式路径 emit ContextUsage

**Files:**
- Modify: `agent/src/streaming.rs`

- [ ] **Step 1: 扩展 `MultiTurnStreamItem`**

```rust
ContextUsage(crate::context_usage::ContextUsageSnapshot),
```

- [ ] **Step 2: 在 `run_multi_turn_stream` 每次拿到 `(history, tools)` 之后、`stream_chat` 之前 emit**

```rust
{
    let agent = session.lock().await;
    let (system_chars, memory_chars, skills_chars, recall_chars) =
        agent.system_prompt_layer_chars();
    let snap = crate::context_usage::build_snapshot(crate::context_usage::ContextUsageInput {
        system_chars,
        memory_chars,
        skills_chars,
        recall_chars,
        tools_json: &tools,
        messages: &history,
        context_window: 0, // 前端用模型窗口覆盖
        updated_at_ms: chrono::Utc::now().timestamp_millis(),
    });
    drop(agent);
    let _ = tx
        .send(Ok(MultiTurnStreamItem::ContextUsage(snap)))
        .await;
}
```

确认 `tools` 类型：`schemas_for_api()` 若返回 `Vec<Value>`，则 `tools_json: &serde_json::Value::Array(tools.clone())` 或改 `build_snapshot` 接受 `&[Value]`。优先改签名为：

```rust
pub tools: &'a [serde_json::Value],
```

并在 `build_snapshot` 内直接 `for t in input.tools`。

- [ ] **Step 3: 匹配穷尽处补上 `ContextUsage` 臂（若有）**

- [ ] **Step 4: 编译**

Run: `cargo check -p agent -p backend`  
Expected: 可能 backend 因未映射而 fail——Task 4 接上。可先只 `cargo check -p agent`。

- [ ] **Step 5: Commit**

```bash
git add agent/src/streaming.rs agent/src/context_usage.rs
git commit -m "$(cat <<'EOF'
feat(agent): emit context usage before each API round

EOF
)"
```

---

### Task 4: Proto → backend → Tauri

**Files:**
- Modify: `proto/proto/astro.proto`
- Modify: `backend/src/grpc/astro_service.rs`
- Modify: `frontend/src-tauri/src/commands.rs`

- [ ] **Step 1: Proto 追加**

```protobuf
message ContextUsageSegment {
  string id = 1;
  uint32 tokens = 2;
  uint32 count = 3; // 0 = 未设置
}

message ContextUsageEvent {
  uint32 context_window = 1;
  uint32 total_tokens = 2;
  repeated ContextUsageSegment segments = 3;
  int64 updated_at = 4;
}
```

在 `ChatEvent.oneof payload` 增加：

```protobuf
ContextUsageEvent context_usage = 13;
```

- [ ] **Step 2: 重新编译 proto**

Run: `cargo build -p proto`  
Expected: OK（build.rs 生成）

- [ ] **Step 3: `multi_turn_to_chat_event` 映射**

```rust
MultiTurnStreamItem::ContextUsage(snap) => Some(ChatEvent {
    payload: Some(proto::chat_event::Payload::ContextUsage(
        proto::ContextUsageEvent {
            context_window: snap.context_window,
            total_tokens: snap.total_tokens,
            segments: snap
                .segments
                .into_iter()
                .map(|s| proto::ContextUsageSegment {
                    id: s.id,
                    tokens: s.tokens,
                    count: s.meta.and_then(|m| m.count).unwrap_or(0),
                })
                .collect(),
            updated_at: snap.updated_at,
        },
    )),
}),
```

- [ ] **Step 4: Tauri `ChatStreamEvent`**

```rust
ContextUsage {
    context_window: u32,
    total_tokens: u32,
    segments: Vec<ContextUsageSegmentDto>,
    updated_at: i64,
},

#[derive(Clone, Serialize)]
pub struct ContextUsageSegmentDto {
    pub id: String,
    pub tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}
```

emit 分支：`count == 0` → `None`。

Serialize 字段名：现有 Usage 用 `prompt_tokens` snake；前端 listen 若按 camel 需确认。检查现有事件：前端用 `promptTokens` 还是 `prompt_tokens`？

在 `App.tsx` 搜 `prompt_tokens` / `promptTokens`——按**现有惯例**对齐（若为 serde rename camelCase 则 DTO 加 `#[serde(rename_all = "camelCase")]`）。

- [ ] **Step 5: 编译**

Run: `cargo check -p backend` 与 `cargo check -p astro-frontend`（或 `frontend/src-tauri` package 名）  
Expected: OK

- [ ] **Step 6: Commit**

```bash
git add proto/proto/astro.proto backend/src/grpc/astro_service.rs frontend/src-tauri/src/commands.rs
git commit -m "$(cat <<'EOF'
feat: pipe context usage events to the desktop UI

EOF
)"
```

---

### Task 5: 前端纯函数库

**Files:**
- Create: `apps/desktop/src/lib/contextUsage.ts`
- Create: `apps/desktop/src/lib/contextUsage.test.ts`

- [ ] **Step 1: 写失败测试**

```ts
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  formatTokenCount,
  usagePercent,
  visibleSegments,
  SEGMENT_ORDER,
  type ContextUsageSnapshot,
} from "./contextUsage.ts";

test("formatTokenCount", () => {
  assert.equal(formatTokenCount(498), "498");
  assert.equal(formatTokenCount(9800), "9.8K");
  assert.equal(formatTokenCount(128_000), "128K");
});

test("usagePercent caps at 99 for bar label", () => {
  assert.equal(usagePercent(0, 128_000), 0);
  assert.equal(usagePercent(23_040, 128_000), 18);
  assert.equal(usagePercent(128_000, 128_000), 99);
});

test("visibleSegments drops zeros and sorts by tokens desc", () => {
  const snap: ContextUsageSnapshot = {
    contextWindow: 128_000,
    totalTokens: 30,
    segments: [
      { id: "system", tokens: 10 },
      { id: "tools", tokens: 0 },
      { id: "conversation", tokens: 20 },
    ],
    updatedAt: 1,
  };
  assert.deepEqual(
    visibleSegments(snap).map((s) => s.id),
    ["conversation", "system"],
  );
});
```

- [ ] **Step 2: 跑测失败**

Run: `cd frontend && node --experimental-strip-types --test src/lib/contextUsage.test.ts`  
Expected: FAIL cannot find module

- [ ] **Step 3: 实现 `contextUsage.ts`**

```ts
export type ContextUsageSegmentId =
  | "system"
  | "tools"
  | "mcp"
  | "memory"
  | "skills"
  | "recall"
  | "subagent"
  | "conversation";

export type ContextUsageSegment = {
  id: ContextUsageSegmentId | string;
  tokens: number;
  count?: number;
};

export type ContextUsageSnapshot = {
  contextWindow: number;
  totalTokens: number;
  segments: ContextUsageSegment[];
  updatedAt: number;
};

export const SEGMENT_ORDER: ContextUsageSegmentId[] = [
  "system",
  "tools",
  "mcp",
  "memory",
  "skills",
  "recall",
  "subagent",
  "conversation",
];

/** CSS 变量名（不含 var()） */
export const SEGMENT_TONE: Record<string, string> = {
  system: "--ink-mute",
  tools: "--tone-purple",
  mcp: "--tone-pink",
  memory: "--tone-green",
  skills: "--tone-amber",
  recall: "--tone-cyan",
  subagent: "--tone-indigo",
  conversation: "--tone-orange",
};

export function formatTokenCount(n: number): string {
  if (!Number.isFinite(n) || n < 0) return "0";
  if (n < 1000) return String(Math.round(n));
  if (n < 1_000_000) {
    const k = n / 1000;
    const s = k >= 100 || Number.isInteger(k) ? String(Math.round(k)) : k.toFixed(1);
    return `${s.replace(/\.0$/, "")}K`;
  }
  const m = n / 1_000_000;
  return `${Number.isInteger(m) ? m : m.toFixed(1)}M`;
}

export function usagePercent(used: number, window: number): number {
  if (window <= 0 || used <= 0) return 0;
  return Math.min(99, Math.round((used / window) * 100));
}

export function visibleSegments(snap: ContextUsageSnapshot): ContextUsageSegment[] {
  return snap.segments
    .filter((s) => s.tokens > 0)
    .slice()
    .sort((a, b) => b.tokens - a.tokens);
}

export function resolveContextWindow(
  modelWindow: number | null | undefined,
  snapWindow: number | null | undefined,
): number {
  if (modelWindow && modelWindow > 0) return modelWindow;
  if (snapWindow && snapWindow > 0) return snapWindow;
  return 128_000;
}
```

- [ ] **Step 4: 跑测通过**

Run: `cd frontend && node --experimental-strip-types --test src/lib/contextUsage.test.ts`  
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/lib/contextUsage.ts apps/desktop/src/lib/contextUsage.test.ts
git commit -m "$(cat <<'EOF'
feat(frontend): add context usage formatting helpers

EOF
)"
```

---

### Task 6: `ContextUsageBar` + Popover + Composer 接线

**Files:**
- Create: `apps/desktop/src/components/ContextUsageBar.tsx`
- Create: `apps/desktop/src/components/ContextUsagePopover.tsx`
- Modify: `apps/desktop/src/components/ChatView.tsx`
- Modify: `apps/desktop/src/styles/chat.css`
- Modify: `apps/desktop/src/i18n/messages.ts`

- [ ] **Step 1: `ContextUsageBar`**

```tsx
import { SEGMENT_TONE, visibleSegments, type ContextUsageSnapshot } from "../lib/contextUsage";

export default function ContextUsageBar({
  snapshot,
  windowTokens,
}: {
  snapshot: ContextUsageSnapshot;
  windowTokens: number;
}) {
  const segs = visibleSegments(snapshot);
  const used = Math.max(snapshot.totalTokens, 1);
  return (
    <div className="ctx-usage-bar" role="img" aria-hidden>
      {segs.map((s) => (
        <span
          key={s.id}
          className="ctx-usage-bar-seg"
          style={{
            flexGrow: s.tokens,
            background: `var(${SEGMENT_TONE[s.id] ?? "--accent"})`,
            maxWidth: `${(s.tokens / windowTokens) * 100}%`,
          }}
        />
      ))}
      <span
        className="ctx-usage-bar-rest"
        style={{ flexGrow: Math.max(0, windowTokens - snapshot.totalTokens) }}
      />
    </div>
  );
}
```

（布局用 `display:flex`；rest 为剩余窗口。若 `total > window`，rest 为 0。）

- [ ] **Step 2: `ContextUsagePopover`**

含标题、百分比、`~{formatTokenCount(used)} / {formatTokenCount(window)}`、`ContextUsageBar`、分项简表、「查看详情」按钮、关闭。空快照时显示 placeholder 文案。

Props：`snapshot | null`、`windowTokens`、`onClose`、`onViewDetails`、`open`。

- [ ] **Step 3: ChatView**

- 将 composer 上下文按钮改为 toggle 浮层（不再直接 `onOpenContext`，或保留：popover 内「查看详情」才 `onOpenContext`）。
- `contextUsagePercent` 来自 snap；无 snap 时可不显示 % 数字。
- 传入新 prop：`contextUsage: ContextUsageSnapshot | null`、`contextWindow`。

- [ ] **Step 4: i18n 键**

```ts
"chat.contextUsage": ... // 已有
"chat.contextUsageDetail": "查看详情",
"chat.contextUsageEmpty": "发送对话后可查看分层占用",
"chat.contextUsageFull": "{pct}% 已用",
"chat.contextSeg.system": "系统提示",
"chat.contextSeg.tools": "工具定义",
"chat.contextSeg.mcp": "MCP 与动态工具",
"chat.contextSeg.memory": "记忆与画像",
"chat.contextSeg.skills": "Skills",
"chat.contextSeg.recall": "动态召回",
"chat.contextSeg.subagent": "子 Agent 返回",
"chat.contextSeg.conversation": "对话消息",
```

英文对称添加。

- [ ] **Step 5: CSS**（`chat.css`）

使用 `--menu-glass-bg`、`--menu-glass-border`、`--menu-glass-shadow`；圆角、padding 对齐现有 popover。

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src/components/ContextUsageBar.tsx \
  apps/desktop/src/components/ContextUsagePopover.tsx \
  apps/desktop/src/components/ChatView.tsx \
  apps/desktop/src/styles/chat.css \
  apps/desktop/src/i18n/messages.ts
git commit -m "$(cat <<'EOF'
feat(chat): add context usage popover on composer

EOF
)"
```

---

### Task 7: ContextExplorer + 右栏

**Files:**
- Create: `apps/desktop/src/components/ContextExplorer.tsx`
- Modify: `apps/desktop/src/components/ChatRightPanel.tsx`
- Modify: `apps/desktop/src/styles/chat-right-panel.css`
- Modify: `apps/desktop/src/i18n/messages.ts`

- [ ] **Step 1: `ContextExplorer` UI**

结构（参考 Context Explorer）：
1. 指标行：`sessionLabel`、窗口、`~tokens`
2. 说明段落（i18n）
3. SVG 环形图：可见 segment 用 `stroke-dasharray`；中心百分比
4. 列表头：标题 + 「展开全部」
5. 可展开行：色点、标签、`(count)`、`~tokens`；展开体可为简短说明或暂空

环形：圆周 `C = 2 * π * r`；每段 `(tokens/window)*C`；剩余用 mute 色。

- [ ] **Step 2: ChatRightPanel**

```tsx
{tab === "context" && (
  <>
    <ContextExplorer
      snapshot={contextUsage}
      windowTokens={contextWindow}
      sessionLabel={sessionId ?? "—"}
    />
    <ChatContextTimeline messages={messages} />
  </>
)}
```

新增 props：`contextUsage`、`contextWindow`。

- [ ] **Step 3: CSS**

`.ctx-explorer`、`.ctx-donut`、`.ctx-explorer-row` 等；暗/亮依赖现有变量。

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src/components/ContextExplorer.tsx \
  apps/desktop/src/components/ChatRightPanel.tsx \
  apps/desktop/src/styles/chat-right-panel.css \
  apps/desktop/src/i18n/messages.ts
git commit -m "$(cat <<'EOF'
feat(chat): add context explorer to the right panel

EOF
)"
```

---

### Task 8: App 状态与模型窗口

**Files:**
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/components/ChatAgentInfo.tsx`

- [ ] **Step 1: state**

```ts
const [contextUsage, setContextUsage] = useState<ContextUsageSnapshot | null>(null);
```

在 `chat-stream-${sid}` listen 中：

```ts
if (payload.type === "contextUsage" /* 或实际字段 */) {
  setContextUsage(normalizeSnapshot(payload));
}
```

切换 session 时 **保留上一会话策略**：按 spec「切会话可清空」——`setContextUsage(null)` on session change。

- [ ] **Step 2: contextWindow**

从当前选中 provider 模型列表取：

```ts
const contextWindow = resolveContextWindow(
  currentModelInfo?.context_window,
  contextUsage?.contextWindow,
);
```

若 App 尚无 model meta，用已有 `get_cached_provider_models` / ModelPicker 缓存；最小实现：先 `invoke` 一次或复用现有 provider state。找不到则 128000。

- [ ] **Step 3: 百分比**

```ts
contextUsagePercent={
  contextUsage
    ? usagePercent(contextUsage.totalTokens, contextWindow)
    : null
}
```

去掉硬编码 128k 与纯 chars 回落作为 **有 snap 时的主路径**；无 snap 可继续粗估，但 % 旁不假装有分层。

- [ ] **Step 4: 传给 ChatView / ChatRightPanel**

`onOpenContext` 仍设 `tab=context` + open。Popover「查看详情」调同一回调并关闭浮层。

- [ ] **Step 5: ChatAgentInfo**

删除或缩小原三维 usage + 单色进度条；改为一行：

```tsx
<button type="button" className="linkish" onClick={onOpenContextTab}>
  {t("chat.rightPanel.viewContextUsage")}
</button>
```

或显示 `usagePercent` 摘要。

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src/App.tsx apps/desktop/src/components/ChatAgentInfo.tsx
git commit -m "$(cat <<'EOF'
feat(chat): wire context usage snapshot through App

EOF
)"
```

---

### Task 9: 验收与收尾

**Files:**
- Modify: `docs/superpowers/specs/2026-07-15-context-usage-explorer-design.md`（状态 → 已实现）

- [ ] **Step 1: 回归测试**

```bash
cargo test -p agent context_usage
cd frontend && node --experimental-strip-types --test src/lib/contextUsage.test.ts
cargo check -p backend
cd frontend && npx tsc -b --pretty false
```

Expected: all OK

- [ ] **Step 2: 手工验收清单**

1. 发一轮对话后 Composer 出现 %，点开浮层见分段条与简表  
2. 「查看详情」打开右栏上下文 Tab，环图中心 % 与 Composer 一致  
3. 时间线仍在 Explorer 下方  
4. 有 MCP 工具时出现 MCP 段；有过 delegate 后出现子 Agent 段  

- [ ] **Step 3: 更新 spec 状态为已实现并 commit**

```bash
git add docs/superpowers/specs/2026-07-15-context-usage-explorer-design.md
git commit -m "$(cat <<'EOF'
docs: mark context usage explorer spec implemented

EOF
)"
```

---

## Spec coverage (self-review)

| Spec 项 | Task |
|---------|------|
| 8 类归类 + ceil/4 | T1 |
| 组装时落盘 | T2–T3 |
| 事件推前端 | T4 |
| Composer 浮层 | T6 |
| 右栏 Explorer + 时间线 S1 | T7 |
| 查看详情 → context tab | T6 + T8 |
| 模型 context_window | T8 |
| 隐藏 0 / `~` 文案 | T5–T7 |
| Agent tab 弱化 | T8 |
| 不做 Debug / tokenizer / Insights | 未列入 |
| 单测 MCP / delegate | T1 |

无 TBD 占位；`contextUsage` 事件字段名以 Task 4 实测的 serde 惯例为准（snake 或 camel），前后端保持一致。
