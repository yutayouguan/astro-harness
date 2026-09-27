# Chat Timeline Interleave Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 助手气泡内按事件顺序交错渲染思考 / 工具 / A2UI，最终正文置底；流式与 SessionStore 重进顺序一致。

**Architecture:** 前端维护 `ChatMessage.segments`（activity/surface 只存 id，实体仍在 `activities`/`uiSurfaces`）；agent 流式同步拼装同构 JSON，写入 `reasoning_details.astro_timeline_v1`（及 `astro_surfaces_v1`）；`build_chat_history` 还原 `segments`+`uiSurfaces`。无 timeline 的旧消息回退「思考→工具→卡→正文」。

**Tech Stack:** TypeScript + `node:test`、React ChatView、Rust `memory::session::store`、agent streaming、Tauri DTO

**Spec:** `docs/superpowers/specs/2026-07-13-chat-timeline-interleave-design.md`

---

## File Structure

| File | Responsibility |
|------|----------------|
| Create: `apps/desktop/src/lib/chatTimeline.ts` | 纯函数：事件 → 更新 `segments` / `activities` / `uiSurfaces` / `reasoning` |
| Create: `apps/desktop/src/lib/chatTimeline.test.ts` | 交错拼装单测 |
| Modify: `apps/desktop/src/types.ts` | `ChatTimelineSegment`；`ChatMessage.segments`；history DTO |
| Modify: `apps/desktop/src/components/ChatView.tsx` | 有 segments 时逐段渲染；无则分组回退 |
| Modify: `apps/desktop/src/App.tsx` | 流式事件走 `chatTimeline`；restore 映射 segments/surfaces |
| Create: `crates/agent-core/src/timeline.rs` | Rust 侧 `TimelineBuilder` + JSON 合并进 `reasoning_details` |
| Modify: `crates/agent-core/src/lib.rs` | `mod timeline` |
| Modify: `crates/agent-core/src/loop_.rs` | `record_assistant_message_with_tools` 接受 `reasoning_details` |
| Modify: `crates/agent-core/src/streaming.rs` | 维护 builder；落盘时写入 |
| Modify: `crates/agent-memory/src/session/store/mod.rs` | `ChatHistoryMessage.segments` / `ui_surfaces` |
| Modify: `crates/agent-memory/src/session/store/search.rs` | `build_chat_history` 解析 `astro_timeline_v1` / `astro_surfaces_v1` |
| Create: `memory/tests/timeline_history_test.rs`（或扩展 `session_store_test.rs`） | 读写 timeline |
| Modify: `apps/desktop/src-tauri/src/commands.rs` | history DTO 增加 `segments` / `uiSurfaces` |
| Modify: `docs/superpowers/specs/2026-07-13-chat-timeline-interleave-design.md` | Status → Implemented（收尾） |

---

### Task 1: 前端纯函数 `chatTimeline`（TDD）

**Files:**
- Create: `apps/desktop/src/lib/chatTimeline.ts`
- Create: `apps/desktop/src/lib/chatTimeline.test.ts`
- Modify: `apps/desktop/src/types.ts`

- [x] **Step 1: 扩展类型**

在 `apps/desktop/src/types.ts` 的 `ChatMessage` 附近加入：

```ts
export type ChatTimelineSegment =
  | {
      type: "reasoning";
      id: string;
      text: string;
      at: number;
      durationSec?: number;
    }
  | {
      type: "activity";
      id: string;
      at: number;
    }
  | {
      type: "surface";
      id: string;
      at: number;
    };

// ChatMessage 增加：
segments?: ChatTimelineSegment[];
```

- [x] **Step 2: 写失败测试**

`apps/desktop/src/lib/chatTimeline.test.ts`：

```ts
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  applyReasoningDelta,
  applyActivityUpsert,
  applySurfaceUpsert,
  sealOpenReasoning,
} from "./chatTimeline.ts";
import type { ChatMessage } from "../types.ts";

function emptyAssistant(id = "a1"): ChatMessage {
  return { id, role: "assistant", content: "" };
}

test("reasoning then tool then reasoning creates three segments", () => {
  let m = emptyAssistant();
  m = applyReasoningDelta(m, "think1", 100);
  m = applyActivityUpsert(m, {
    id: "c1",
    kind: "tool",
    title: "present_ui",
    status: "running",
    at: 200,
  });
  m = applyReasoningDelta(m, "think2", 300);
  assert.equal(m.segments?.length, 3);
  assert.equal(m.segments?.[0]?.type, "reasoning");
  assert.equal(m.segments?.[1]?.type, "activity");
  assert.equal(m.segments?.[2]?.type, "reasoning");
  assert.equal((m.segments?.[0] as { text: string }).text, "think1");
  assert.equal((m.segments?.[2] as { text: string }).text, "think2");
  assert.equal(m.reasoning, "think1think2");
  assert.equal(m.activities?.length, 1);
});

test("activity upsert same id does not duplicate segment", () => {
  let m = emptyAssistant();
  m = applyActivityUpsert(m, {
    id: "c1",
    kind: "tool",
    title: "x",
    status: "running",
    at: 1,
  });
  m = applyActivityUpsert(m, {
    id: "c1",
    kind: "tool",
    title: "x",
    output: "ok",
    status: "done",
    at: 2,
  });
  assert.equal(m.segments?.filter((s) => s.type === "activity").length, 1);
  assert.equal(m.activities?.[0]?.output, "ok");
});

test("surface after activity appends surface segment", () => {
  let m = emptyAssistant();
  m = applyActivityUpsert(m, {
    id: "c1",
    kind: "tool",
    title: "present_ui",
    at: 1,
  });
  m = applySurfaceUpsert(m, {
    messageId: "a2ui-surface-c1",
    activityType: "a2ui-surface",
    operations: [{ version: "v0.9" }],
    status: "active",
  }, 2);
  assert.equal(m.segments?.at(-1)?.type, "surface");
  assert.equal(m.uiSurfaces?.length, 1);
});
```

- [x] **Step 3: 跑测试确认失败**

Run:

```bash
cd frontend && node --test --experimental-strip-types src/lib/chatTimeline.test.ts
```

Expected: FAIL（模块不存在）

- [x] **Step 4: 实现 `chatTimeline.ts`**

```ts
import type {
  ChatActivity,
  ChatMessage,
  ChatTimelineSegment,
  UiSurface,
} from "../types";

function ensureSegments(m: ChatMessage): ChatTimelineSegment[] {
  return [...(m.segments ?? [])];
}

/** 追加 reasoning；若末段非 reasoning 则新开段；同步拼接 m.reasoning */
export function applyReasoningDelta(
  m: ChatMessage,
  delta: string,
  at: number = Date.now(),
): ChatMessage {
  if (!delta) return m;
  const segments = ensureSegments(m);
  const last = segments[segments.length - 1];
  if (last?.type === "reasoning") {
    segments[segments.length - 1] = {
      ...last,
      text: last.text + delta,
    };
  } else {
    segments.push({
      type: "reasoning",
      id: `r-${at}-${segments.length}`,
      text: delta,
      at,
    });
  }
  return {
    ...m,
    segments,
    reasoning: (m.reasoning ?? "") + delta,
  };
}

/** 工具活动 upsert；新 id 时 push activity 段（先封口 reasoning 段逻辑由调用方或本函数保证） */
export function applyActivityUpsert(
  m: ChatMessage,
  activity: ChatActivity,
): ChatMessage {
  const at = activity.at ?? Date.now();
  let segments = ensureSegments(m);
  const last = segments[segments.length - 1];
  if (last?.type === "reasoning") {
    // 保持段边界：不自动改 duration；仅确保下一段是 activity
  }
  const activities = [...(m.activities ?? [])];
  const idx = activities.findIndex((a) => a.id === activity.id);
  if (idx >= 0) {
    activities[idx] = { ...activities[idx], ...activity, at: activities[idx].at ?? at };
  } else {
    activities.push({ ...activity, at });
    segments.push({ type: "activity", id: activity.id, at });
  }
  return { ...m, activities, segments };
}

export function applySurfaceUpsert(
  m: ChatMessage,
  surface: UiSurface,
  at: number = Date.now(),
): ChatMessage {
  const surfaces = [...(m.uiSurfaces ?? [])];
  const idx = surfaces.findIndex((s) => s.messageId === surface.messageId);
  if (idx >= 0) surfaces[idx] = surface;
  else surfaces.push(surface);

  let segments = ensureSegments(m);
  const hasSeg = segments.some(
    (s) => s.type === "surface" && s.id === surface.messageId,
  );
  if (!hasSeg) {
    segments.push({ type: "surface", id: surface.messageId, at });
  }
  return { ...m, uiSurfaces: surfaces, segments };
}

export function sealOpenReasoning(
  m: ChatMessage,
  durationSec?: number,
): ChatMessage {
  const segments = ensureSegments(m);
  const last = segments[segments.length - 1];
  if (last?.type !== "reasoning") return m;
  segments[segments.length - 1] = {
    ...last,
    durationSec: durationSec ?? last.durationSec,
  };
  return { ...m, segments };
}
```

- [x] **Step 5: 跑测试确认通过**

Run:

```bash
cd frontend && node --test --experimental-strip-types src/lib/chatTimeline.test.ts
```

Expected: PASS

- [x] **Step 6: Commit**

```bash
git add apps/desktop/src/types.ts apps/desktop/src/lib/chatTimeline.ts apps/desktop/src/lib/chatTimeline.test.ts
git commit -m "$(cat <<'EOF'
feat(chat): add timeline segment helpers for interleaved turns

EOF
)"
```

---

### Task 2: ChatView 按 segments 渲染

**Files:**
- Modify: `apps/desktop/src/components/ChatView.tsx`

- [x] **Step 1: 在助手气泡内分支渲染**

找到当前 `m.activities` / `m.uiSurfaces` / `MsgReasoning` / `ChatMarkdown` 区块，改为：

```tsx
{m.segments && m.segments.length > 0 ? (
  <>
    {m.segments.map((seg) => {
      if (seg.type === "reasoning") {
        const isLast =
          m.segments![m.segments!.length - 1] === seg;
        const active = Boolean(
          isStreamingBubble && isLast && !m.content,
        );
        return (
          <MsgReasoning
            key={seg.id}
            reasoning={seg.text}
            active={active}
            durationSec={seg.durationSec}
          />
        );
      }
      if (seg.type === "activity") {
        const act = m.activities?.find((a) => a.id === seg.id);
        if (!act) return null;
        return (
          <ActivityCards
            key={seg.id}
            items={[act]}
            prefs={displayPrefs}
            showTimestamps={displayPrefs.showTimestamps}
          />
        );
      }
      const surface = m.uiSurfaces?.find(
        (s) => s.messageId === seg.id,
      );
      if (!surface) return null;
      return (
        <A2UIRenderer
          key={seg.id}
          operations={surface.operations}
          disabled={surface.status !== "active"}
          onAction={(name, context) =>
            onUiAction?.(m.id, name, context)
          }
        />
      );
    })}
  </>
) : (
  <>
    {/* 回退：思考 → 工具组 → A2UI（保持现有顺序） */}
    {m.reasoning ? (
      <MsgReasoning
        reasoning={m.reasoning}
        active={reasoningActive}
        durationSec={m.reasoningDurationSec}
      />
    ) : null}
    {m.activities && m.activities.length > 0 && (
      <ActivityCards
        items={m.activities}
        prefs={displayPrefs}
        showTimestamps={displayPrefs.showTimestamps}
      />
    )}
    {m.uiSurfaces?.map((surface) => (
      <A2UIRenderer
        key={surface.messageId}
        operations={surface.operations}
        disabled={surface.status !== "active"}
        onAction={(name, context) =>
          onUiAction?.(m.id, name, context)
        }
      />
    ))}
  </>
)}
{/* 正文始终在后 */}
{m.content && (
  <ChatMarkdown ... />
)}
```

注意：attachments 仍在最前；token stats 仍在最后。有 segments 时 **不要** 再包一层父级 `ActivityGroup` 整包折叠（`ActivityCards` 单条即可）。

- [x] **Step 2: 类型检查**

Run:

```bash
cd frontend && ./node_modules/.bin/tsc -b --pretty false
```

Expected: 无错误

- [x] **Step 3: Commit**

```bash
git add apps/desktop/src/components/ChatView.tsx
git commit -m "$(cat <<'EOF'
feat(chat): render assistant bubble from timeline segments

EOF
)"
```

---

### Task 3: App 流式接线

**Files:**
- Modify: `apps/desktop/src/App.tsx`

- [x] **Step 1: import helpers**

```ts
import {
  applyActivityUpsert,
  applyReasoningDelta,
  applySurfaceUpsert,
  sealOpenReasoning,
} from "./lib/chatTimeline";
```

- [x] **Step 2: `flushStreamTokens` 里对 reasoning 批处理改走 `applyReasoningDelta`**

在合并 `reasoningBatch` 时，对每条消息：

```ts
let next = m;
const reasoningExtra = reasoningBatch.get(m.id);
if (reasoningExtra) {
  next = applyReasoningDelta(next, reasoningExtra, Date.now());
}
// content extra 仍拼到 content
```

- [x] **Step 3: `flushToolDeltas` / `tool_call` / `memory_update` 用 `applyActivityUpsert`**

创建或更新 activity 后：

```ts
return applyActivityUpsert(
  { ...m, activities }, // 或直接传 activity
  activity,
);
```

优先：构造完整 `ChatActivity` 后只调用 `applyActivityUpsert(m, activity)`，避免双写。

- [x] **Step 4: `activity`（A2UI）事件用 `applySurfaceUpsert`**

替换直接 `uiSurfaces` push 的逻辑。

- [x] **Step 5: `done` / interrupt 结束时 `sealOpenReasoning`**

用 `reasoningStartRef` 计算 `durationSec` 后：

```ts
next = sealOpenReasoning(next, durationSec);
```

- [x] **Step 6: tsc**

Run: `cd frontend && ./node_modules/.bin/tsc -b --pretty false`  
Expected: 干净

- [x] **Step 7: Commit**

```bash
git add apps/desktop/src/App.tsx
git commit -m "$(cat <<'EOF'
feat(chat): build timeline segments during stream events

EOF
)"
```

---

### Task 4: Agent `TimelineBuilder` + 落盘

**Files:**
- Create: `crates/agent-core/src/timeline.rs`
- Modify: `crates/agent-core/src/lib.rs`
- Modify: `crates/agent-core/src/loop_.rs`
- Modify: `crates/agent-core/src/streaming.rs`
- Test: `crates/agent-core/src/timeline.rs` 内 `#[cfg(test)]` 或 `agent/tests/timeline_test.rs`

- [x] **Step 1: 实现 `timeline.rs`**

```rust
use serde_json::{json, Value};

#[derive(Debug, Clone, Default)]
pub struct TimelineBuilder {
    segments: Vec<Value>,
    surfaces: Vec<Value>,
}

impl TimelineBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_reasoning_delta(&mut self, delta: &str, at_ms: i64) {
        if delta.is_empty() {
            return;
        }
        if let Some(last) = self.segments.last_mut() {
            if last.get("type").and_then(|t| t.as_str()) == Some("reasoning") {
                let text = last.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string()
                    + delta;
                last.as_object_mut().unwrap().insert("text".into(), json!(text));
                return;
            }
        }
        self.segments.push(json!({
            "type": "reasoning",
            "id": format!("r-{at_ms}-{}", self.segments.len()),
            "text": delta,
            "at": at_ms,
        }));
    }

    pub fn upsert_activity(&mut self, id: &str, at_ms: i64) {
        let exists = self.segments.iter().any(|s| {
            s.get("type").and_then(|t| t.as_str()) == Some("activity")
                && s.get("id").and_then(|t| t.as_str()) == Some(id)
        });
        if !exists {
            self.segments.push(json!({
                "type": "activity",
                "id": id,
                "at": at_ms,
            }));
        }
    }

    pub fn upsert_surface(&mut self, surface: Value, at_ms: i64) {
        let sid = surface
            .get("messageId")
            .or_else(|| surface.get("message_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if sid.is_empty() {
            return;
        }
        if let Some(pos) = self.surfaces.iter().position(|s| {
            s.get("messageId").and_then(|v| v.as_str()) == Some(sid.as_str())
        }) {
            self.surfaces[pos] = surface;
        } else {
            self.surfaces.push(surface);
            self.segments.push(json!({
                "type": "surface",
                "id": sid,
                "at": at_ms,
            }));
        }
    }

    /// 合并进已有 reasoning_details 对象。
    pub fn into_reasoning_details(self, existing: Option<Value>) -> Value {
        let mut obj = match existing {
            Some(Value::Object(m)) => m,
            _ => serde_json::Map::new(),
        };
        obj.insert("astro_timeline_v1".into(), Value::Array(self.segments));
        if !self.surfaces.is_empty() {
            obj.insert("astro_surfaces_v1".into(), Value::Array(self.surfaces));
        }
        Value::Object(obj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaves_reasoning_and_activity() {
        let mut b = TimelineBuilder::new();
        b.push_reasoning_delta("a", 1);
        b.upsert_activity("c1", 2);
        b.push_reasoning_delta("b", 3);
        let v = b.into_reasoning_details(None);
        let segs = v["astro_timeline_v1"].as_array().unwrap();
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0]["type"], "reasoning");
        assert_eq!(segs[1]["type"], "activity");
        assert_eq!(segs[2]["text"], "b");
    }
}
```

- [x] **Step 2: 跑测**

Run: `cargo test -p agent timeline -- --nocapture`  
Expected: PASS

- [x] **Step 3: 扩展 `record_assistant_message_with_tools`**

```rust
pub fn record_assistant_message_with_tools(
    &mut self,
    content: &str,
    tool_calls: Option<Vec<common::message::ToolCall>>,
    reasoning: Option<&str>,
    reasoning_details: Option<serde_json::Value>,
) -> anyhow::Result<()> {
    // ...
    self.memory.record_message_ex(
        &self.session_id,
        NewMessage {
            content: Some(content),
            tool_calls: tool_calls_json,
            reasoning,
            reasoning_details,
            ..NewMessage::empty(&self.session_id, "assistant")
        },
    )?;
    // session_messages 镜像不变
}
```

更新所有调用点：无 details 传 `None`；`record_assistant_message` 转调时 `None`。

- [x] **Step 4: `streaming.rs` 维护 `TimelineBuilder`**

在 `run_multi_turn_stream` 每轮：

```rust
let mut timeline = crate::timeline::TimelineBuilder::new();
let now_ms = || chrono::chrono::Utc::now().timestamp_millis();
```

- 收到 `Reasoning(r)`：`timeline.push_reasoning_delta(&r, now_ms());`
- 开始/确认 tool call id：`timeline.upsert_activity(&call.id, now_ms());`
- 解析到 `astro_ui` / HITL activity 发 Activity 时：

```rust
timeline.upsert_surface(json!({
  "messageId": message_id,
  "activityType": "a2ui-surface",
  "operations": operations,
  "status": "active",
}), now_ms());
```

- `record_assistant_message_with_tools(..., Some(timeline.clone().into_reasoning_details(None)))`  
  注意：多轮工具时每轮 assistant（带 tool_calls）都要写入当时 timeline 快照；最终纯文本 assistant 行写入完整 timeline。

简化策略（与 spec 一致、足够验收）：

- **仅在最终成功/中断结束前的最后一次 assistant 文本落盘**带完整 timeline；带 `tool_calls` 的中间 assistant 行 `reasoning_details` 可为阶段性快照或 `None`。  
- **推荐：** 每次 `record_assistant_message_with_tools` 都写入 **当前** `into_reasoning_details` 克隆，history 折叠时取 **该会话最后一条带 `astro_timeline_v1` 的 assistant** 的 timeline 挂到最终气泡（见 Task 5）。

  > **勘误（705ddb5f）：** 上面「history 折叠时取最后一条带 `astro_timeline_v1` 的 assistant 挂到最终气泡」已废弃。按此实现出的 `patch_last_assistant_timeline`（工具循环后回写「最近一条 assistant message 行」）会把**本轮**时间线写到**上一轮**回答行：回放时 activity 段全部找不到活动，只剩一串「思考完成」。现行实现：每轮记录时把本轮时间线写进本轮 assistant item（原生 Responses 路径由 `runtime::recording::attach_assistant_timeline_metadata` 补挂；`types::model_tool::merge_google_thought_signature` 在无 Google 签名时不再丢弃整份 `reasoning_details`）；前端对历史错行时间线按 activity 归属重新安置，找不到归属才回退分组视图。

- [x] **Step 5: Commit**

```bash
git add agent/src/timeline.rs agent/src/lib.rs agent/src/loop_.rs agent/src/streaming.rs
git commit -m "$(cat <<'EOF'
feat(agent): build and persist astro_timeline_v1 on assistant turns

EOF
)"
```

---

### Task 5: SessionStore history 还原 + Tauri DTO

**Files:**
- Modify: `crates/agent-memory/src/session/store/mod.rs`
- Modify: `crates/agent-memory/src/session/store/search.rs`
- Modify: `memory/tests/session_store_test.rs`（或新建 `timeline_history_test.rs`）
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src/types.ts`（history DTO）
- Modify: `apps/desktop/src/App.tsx`（`mapHistoryMessages`）

- [x] **Step 1: 扩展 `ChatHistoryMessage`**

```rust
pub struct ChatHistoryMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub reasoning: Option<String>,
    pub activities: Vec<ChatActivityStored>,
    pub segments: Option<Value>,      // astro_timeline_v1 array
    pub ui_surfaces: Option<Value>,   // astro_surfaces_v1 array
}
```

所有构造处补 `segments: None, ui_surfaces: None`。

- [x] **Step 2: `build_chat_history` 在 assistant 分支**

```rust
let (segments, ui_surfaces) = match &m.reasoning_details {
    Some(Value::Object(map)) => (
        map.get("astro_timeline_v1").cloned(),
        map.get("astro_surfaces_v1").cloned(),
    ),
    _ => (None, None),
};
```

当后续 `tool` 行合并进同一 assistant 气泡时，**保留**该 assistant 已带的 segments/surfaces（不要清掉）。若同一轮有多条 assistant（tool_calls 后还有最终文本），折叠策略：

- UI 气泡以 **最后一条** 有 content 或有 timeline 的 assistant 为准；  
- 实现上：`tool` 结果仍挂到最近 assistant；若新 assistant 推入且带 `astro_timeline_v1`，用新的覆盖展示字段。

单测覆盖：一条 assistant(tool_calls+timeline) + tool + assistant(final+full timeline) → history 一条气泡且 segments 为完整版。

- [x] **Step 3: 测试**

```rust
#[test]
fn build_chat_history_restores_timeline() {
    // append user; append assistant with reasoning_details astro_timeline_v1;
    // build_chat_history; assert segments len / order
}
```

Run: `cargo test -p memory --test session_store_test timeline -- --nocapture`  
Expected: PASS

- [x] **Step 4: Tauri DTO**

```rust
pub struct ChatHistoryMessageDto {
    // ...
    pub segments: Option<serde_json::Value>,
    pub ui_surfaces: Option<serde_json::Value>,
}
```

映射时传入 `m.segments` / `m.ui_surfaces`。

- [x] **Step 5: 前端 `mapHistoryMessages`**

```ts
segments: Array.isArray(m.segments) ? m.segments : undefined,
uiSurfaces: Array.isArray(m.uiSurfaces)
  ? m.uiSurfaces.map(...)
  : undefined,
```

（字段名以 camelCase DTO 为准：`uiSurfaces`。）

- [x] **Step 6: Commit**

```bash
git add memory apps/desktop/src-tauri/src/commands.rs apps/desktop/src/App.tsx apps/desktop/src/types.ts
git commit -m "$(cat <<'EOF'
feat(session): restore chat timeline segments from reasoning_details

EOF
)"
```

---

### Task 6: 回归与文档收尾

**Files:**
- Modify: `docs/superpowers/specs/2026-07-13-chat-timeline-interleave-design.md`

- [x] **Step 1: 自动化**

```bash
cd frontend && node --test --experimental-strip-types src/lib/chatTimeline.test.ts
cd frontend && ./node_modules/.bin/tsc -b --pretty false
cargo test -p agent timeline
cargo test -p memory --test session_store_test
```

Expected: 全绿

- [x] **Step 2: 手工清单**

1. DeepSeek 思考模型：想→工具→再想→正文，气泡交错  
2. `present_ui` 卡插在对应工具附近  
3. 重启 App / 清 localStorage 后从会话恢复顺序不变  
4. 无 timeline 的旧消息仍可读  

- [x] **Step 3: Spec 状态**

`**状态:** 已批准 / 已实现`

- [x] **Step 4: Commit**

```bash
git add docs/superpowers/specs/2026-07-13-chat-timeline-interleave-design.md
git commit -m "$(cat <<'EOF'
docs: mark chat timeline interleave design implemented

EOF
)"
```

---

## Spec coverage (self-review)

| Spec 要求 | Task |
|-----------|------|
| segments 类型 + 引用 activity/surface | 1 |
| 流式拼装 | 1, 3 |
| ChatView 交错 + 正文置底 | 2 |
| 无 segments 回退 | 2 |
| Agent TimelineBuilder + reasoning_details | 4 |
| SessionStore / history / Tauri / restore | 5 |
| 测试 + 验收 + 文档 | 6 |
| 不做 interim 正文 / hermes 命名 | 全任务遵守 |

## Placeholder / Consistency Self-Review

- 无 TBD；surface 持久化明确为 `astro_surfaces_v1`（补强 spec「仅 id 引用」在 DB 侧的可恢复性）。  
- 前后端字段名：`astro_timeline_v1` / `astro_surfaces_v1` 一致。  
- `record_assistant_message_with_tools` 新参数在所有调用点补齐。

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-13-chat-timeline-interleave.md`.

**两种执行方式：**

1. **Subagent-Driven（推荐）** — 每任务新开子代理，任务间复审  
2. **Inline Execution** — 本会话按 executing-plans 连续执行并设检查点  

选哪一种？
