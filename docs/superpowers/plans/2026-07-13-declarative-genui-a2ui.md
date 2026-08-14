# Declarative GenUI (A2UI + AG-UI) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在现有 gRPC→Tauri 聊天流上对齐 AG-UI 语义，用 A2UI 渲染澄清/确认/信息卡，HITL 走真 interrupt/resume。

**Architecture:** 新增 `a2ui` crate（类型 + catalog 校验 + HITL 模板）；扩展 `ChatEvent`/`MultiTurnStreamItem`/`ChatStreamEvent`；Agent 增加 interrupt 状态机；前端 `CatalogAdapter` + `A2UIRenderer`；`confirm`/`clarify` 模板工具触发 interrupt，信息卡仅 `activity`。

**Tech Stack:** Rust (proto/tonic, agent, tools), Tauri 2, React 18 + TypeScript, A2UI v0.9 JSON envelopes

**Spec:** `docs/superpowers/specs/2026-07-13-declarative-genui-a2ui-design.md`  
**Mapping:** `docs/superpowers/specs/2026-07-13-agui-a2ui-astro-mapping.md`

---

## File Structure

| File | Responsibility |
|------|----------------|
| Create: `a2ui/Cargo.toml`, `a2ui/src/lib.rs` | A2UI 类型、catalog 常量、校验、HITL 模板构建 |
| Create: `a2ui/tests/validate_test.rs` | 校验与模板单测 |
| Modify: `Cargo.toml` | workspace member `a2ui` |
| Modify: `proto/proto/astro.proto` | `ActivityEvent`、`RunStarted`/`RunFinished`、`InterruptResume`、控制动作分流 |
| Modify: `crates/agent-core/src/streaming.rs` | `MultiTurnStreamItem::{RunStarted, Activity, RunFinished}` |
| Create: `crates/agent-core/src/interrupt.rs` | 挂起/校验 resume/取消 状态机 |
| Modify: `crates/agent-core/src/lib.rs` | 导出 interrupt |
| Modify: `crates/agent-server/src/grpc/astro_service.rs` | 事件映射；pending interrupt 闸门；`InterruptResume` RPC |
| Modify: `crates/agent-tools/src/builtin/confirm.rs` (new) + `clarify.rs` | 模板 A2UI JSON + interrupt 标记结果 |
| Modify: `crates/agent-tools/src/builtin/mod.rs`, `dispatch.rs`, `lib.rs` | 注册 confirm |
| Modify: `apps/desktop/src-tauri/src/commands.rs` | `ChatStreamEvent` 新变体；`interrupt_resume`；`stream_resume` 别名 |
| Modify: `apps/desktop/src/types.ts` | `UiSurface`, `ChatMessage.uiSurfaces`, interrupt 类型 |
| Create: `apps/desktop/src/a2ui/types.ts` | 前端 A2UI operation 类型 |
| Create: `apps/desktop/src/a2ui/CatalogAdapter.tsx` | 组件 → React |
| Create: `apps/desktop/src/a2ui/A2UIRenderer.tsx` | surface 渲染 + action 回调 |
| Create: `apps/desktop/src/a2ui/validate.ts` | 轻量前端校验（未知组件降级） |
| Modify: `apps/desktop/src/components/ChatView.tsx` | 插入 `uiSurfaces` |
| Modify: `apps/desktop/src/App.tsx` | 消费新事件；interrupt_resume；拒发普通消息 |
| Modify: `apps/desktop/src/styles/chat.css` | A2UI 卡样式 |
| Modify: `apps/desktop/src/i18n/messages.ts` | 文案 |
| Create: `crates/agent-tools/src/builtin/present_ui.rs` | 信息卡：校验后返回 A2UI（不 interrupt） |
| Modify: session/history 相关（见 Task 11） | 持久化 surfaces / pending interrupts |

---

### Task 1: `a2ui` crate — 类型与 catalog 校验

**Files:**
- Create: `a2ui/Cargo.toml`
- Create: `a2ui/src/lib.rs`
- Create: `a2ui/src/validate.rs`
- Create: `a2ui/src/catalog.rs`
- Create: `a2ui/tests/validate_test.rs`
- Modify: `Cargo.toml` (workspace members)

- [ ] **Step 1: 添加 workspace 成员与包**

`Cargo.toml` members 增加 `"a2ui"`。

`a2ui/Cargo.toml`:

```toml
[package]
name = "a2ui"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
```

- [ ] **Step 2: 写失败测试**

`a2ui/tests/validate_test.rs`:

```rust
use a2ui::{validate_operations, ASTRO_CATALOG_ID};

#[test]
fn rejects_unknown_component() {
    let ops = serde_json::json!([
        {
            "version": "v0.9",
            "createSurface": {
                "surfaceId": "s1",
                "catalogId": ASTRO_CATALOG_ID
            }
        },
        {
            "version": "v0.9",
            "updateComponents": {
                "surfaceId": "s1",
                "components": [
                    { "id": "root", "component": "NotARealWidget", "text": "x" }
                ]
            }
        }
    ]);
    let err = validate_operations(ops.as_array().unwrap()).unwrap_err();
    assert!(err.to_string().contains("NotARealWidget"));
}

#[test]
fn accepts_text_card_button() {
    let ops = serde_json::json!([
        {
            "version": "v0.9",
            "createSurface": {
                "surfaceId": "s1",
                "catalogId": ASTRO_CATALOG_ID
            }
        },
        {
            "version": "v0.9",
            "updateComponents": {
                "surfaceId": "s1",
                "components": [
                    { "id": "root", "component": "Card", "child": "col" },
                    { "id": "col", "component": "Column", "children": ["t", "b"] },
                    { "id": "t", "component": "Text", "text": "Hello", "variant": "h2" },
                    {
                        "id": "b",
                        "component": "Button",
                        "child": "bt",
                        "variant": "primary",
                        "action": { "event": { "name": "ok" } }
                    },
                    { "id": "bt", "component": "Text", "text": "OK" }
                ]
            }
        }
    ]);
    validate_operations(ops.as_array().unwrap()).unwrap();
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test -p a2ui --test validate_test`

Expected: FAIL（crate/函数不存在）

- [ ] **Step 4: 最小实现**

`a2ui/src/catalog.rs` — 允许组件集合：

```rust
pub const ASTRO_CATALOG_ID: &str = "astro://a2ui/catalog/v1";

pub const ALLOWED_COMPONENTS: &[&str] = &[
    "Text", "Icon", "Divider", "Card", "Column", "Row", "Button",
    "TextField", "ChoicePicker", "CheckBox", "Image", "List",
];
```

`a2ui/src/validate.rs` — 遍历 operations：

- 每项须有 `version` 与四键之一：`createSurface` / `updateComponents` / `updateDataModel` / `deleteSurface`
- `createSurface.catalogId` 必须为 `ASTRO_CATALOG_ID`（或 basic catalog URL，MVP 只接受 Astro id）
- `updateComponents.components[].component` ∈ `ALLOWED_COMPONENTS`

`a2ui/src/lib.rs` 导出 `validate_operations`、`ASTRO_CATALOG_ID`。

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p a2ui --test validate_test`  
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml a2ui
git commit -m "$(cat <<'EOF'
feat(a2ui): add catalog validation crate

EOF
)"
```

---

### Task 2: HITL A2UI 模板（confirm / clarify）

**Files:**
- Create: `a2ui/src/templates.rs`
- Modify: `a2ui/src/lib.rs`
- Modify: `a2ui/tests/validate_test.rs`（或新 `templates_test.rs`）

- [ ] **Step 1: 写失败测试**

```rust
use a2ui::templates::{build_clarify_surface, build_confirm_surface};
use a2ui::validate_operations;

#[test]
fn confirm_template_validates() {
    let ops = build_confirm_surface(
        "surf-confirm-1",
        "删除文件？",
        "将永久删除 report.pdf",
    );
    validate_operations(&ops).unwrap();
}

#[test]
fn clarify_template_validates() {
    let ops = build_clarify_surface(
        "surf-clarify-1",
        "选哪个环境？",
        &["staging".into(), "production".into()],
    );
    validate_operations(&ops).unwrap();
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p a2ui templates`  
Expected: FAIL

- [ ] **Step 3: 实现模板**

`build_confirm_surface(surface_id, title, body) -> Vec<Value>`：

- `createSurface` + `Card`/`Column`/`Text`×2 + `Row` 内两个 `Button`：`approve` / `deny`
- action.event.name 分别为 `approve` / `deny`

`build_clarify_surface(surface_id, question, options: &[String]) -> Vec<Value>`：

- `Text` 问题 + `ChoicePicker`（options）或每个选项一个 `Button`（MVP 用 Button 列表更简单，避免 ChoicePicker 数据绑定复杂度）
- 提交按钮 `action.event.name = "submit_clarify"`，`context` 含选项值；或每选项按钮 `name = "choose"` + context `{ "value": "..." }`

MVP 推荐：**每选项一个 Button**，payload 直接是所选字符串，降低 dataModel 复杂度。

- [ ] **Step 4: 测试通过并 Commit**

```bash
cargo test -p a2ui
git add a2ui
git commit -m "$(cat <<'EOF'
feat(a2ui): add confirm and clarify surface templates

EOF
)"
```

---

### Task 3: Proto 扩展

**Files:**
- Modify: `proto/proto/astro.proto`
- Regenerate via existing proto build（`cargo build -p proto`）

- [ ] **Step 1: 扩展 proto**

在 `astro.proto` 增加（字段号避开已用）：

```protobuf
message ActivityEvent {
  string message_id = 1;
  string activity_type = 2; // "a2ui-surface"
  string content_json = 3;  // {"operations":[...]}
  bool replace = 4;
}

message RunStartedEvent {
  string thread_id = 1;
  string run_id = 2;
}

message Interrupt {
  string id = 1;
  string reason = 2; // tool_call | input_required | confirmation
  string message = 3;
  string tool_call_id = 4;
  string response_schema_json = 5;
  string expires_at = 6;
  string metadata_json = 7;
}

message RunFinishedEvent {
  string run_id = 1;
  // "success" | "interrupt"
  string outcome_type = 2;
  repeated Interrupt interrupts = 3;
}

// ChatEvent.payload 新增:
//   ActivityEvent activity = 9;
//   RunStartedEvent run_started = 10;
//   RunFinishedEvent run_finished = 11;
// 保留 done = 5 作兼容：成功结束可同时或仅发 run_finished

message InterruptResumeItem {
  string interrupt_id = 1;
  string status = 2; // resolved | cancelled
  string payload_json = 3;
}

message InterruptResumeRequest {
  string session_id = 1;
  repeated InterruptResumeItem resume = 2;
}

// AstroService 新增:
//   rpc InterruptResume(InterruptResumeRequest) returns (Empty);

// ChatControlAction 新增（可选）:
//   CHAT_CONTROL_STREAM_RESUME = 4; // 与旧 RESUME 同义
```

`ChatRequest` 增加可选字段（若同 RPC 续跑）或仅用独立 `InterruptResume` RPC 再调 `Chat`——**MVP：独立 `InterruptResume` 存 pending，随后客户端再调 `Chat` 带空用户消息或专用 flag。**

更清晰的 MVP：

1. `InterruptResume` 校验并写入 session pending resolutions  
2. 客户端立即 `Chat` 且 `ChatRequest.resume_json` 非空 → Agent 消费 resume 后继续  

在 `ChatRequest` 增加：

```protobuf
string resume_json = 19; // JSON array of InterruptResumeItem；有 pending 时必填
```

- [ ] **Step 2: 编译 proto**

Run: `cargo build -p proto`  
Expected: SUCCESS

- [ ] **Step 3: Commit**

```bash
git add proto
git commit -m "$(cat <<'EOF'
feat(proto): add activity, run lifecycle, and interrupt resume

EOF
)"
```

---

### Task 4: Agent interrupt 状态机

**Files:**
- Create: `crates/agent-core/src/interrupt.rs`
- Create: `agent/tests/interrupt_test.rs`
- Modify: `crates/agent-core/src/lib.rs`
- Modify: `agent/Cargo.toml`（依赖 `a2ui` 若需要）

- [ ] **Step 1: 写失败测试**

```rust
use agent::interrupt::{Interrupt, InterruptPending, ResumeItem};

#[test]
fn rejects_partial_resume() {
    let mut p = InterruptPending::new(vec![
        Interrupt { id: "i1".into(), reason: "confirmation".into(), ..Default::default() },
        Interrupt { id: "i2".into(), reason: "confirmation".into(), ..Default::default() },
    ]);
    let err = p
        .apply_resume(&[ResumeItem { interrupt_id: "i1".into(), status: "resolved".into(), payload_json: "{}".into() }])
        .unwrap_err();
    assert!(err.to_string().contains("partial"));
}

#[test]
fn accepts_full_resume() {
    let mut p = InterruptPending::new(vec![
        Interrupt { id: "i1".into(), reason: "confirmation".into(), ..Default::default() },
    ]);
    p.apply_resume(&[ResumeItem {
        interrupt_id: "i1".into(),
        status: "resolved".into(),
        payload_json: r#"{"approved":true}"#.into(),
    }])
    .unwrap();
    assert!(p.is_cleared());
}
```

- [ ] **Step 2: 实现 `InterruptPending`**

- `new(interrupts)`  
- `apply_resume(items)`：必须覆盖全部 id；校验 `response_schema_json`（若非空则用 `jsonschema` 或手写 approve bool）  
- MVP schema：confirm 要求 `{"approved": bool}`；clarify 要求 `{"value": string}`  
- `cancel_all()`  
- `is_cleared()`

- [ ] **Step 3: 测试通过并 Commit**

```bash
cargo test -p agent --test interrupt_test
git add agent
git commit -m "$(cat <<'EOF'
feat(agent): add interrupt pending state machine

EOF
)"
```

---

### Task 5: 扩展 `MultiTurnStreamItem` 与流结束

**Files:**
- Modify: `crates/agent-core/src/streaming.rs`
- Modify: `crates/agent-server/src/grpc/astro_service.rs`（`multi_turn_to_chat_event`）

- [ ] **Step 1: 扩展枚举**

```rust
pub enum MultiTurnStreamItem {
    // ... existing ...
    RunStarted { thread_id: String, run_id: String },
    Activity {
        message_id: String,
        activity_type: String,
        content_json: String,
        replace: bool,
    },
    RunFinished {
        run_id: String,
        outcome_type: String, // "success" | "interrupt"
        interrupts_json: String, // JSON array; empty if success
    },
    // Done 保留：success 路径仍发 Done；interrupt 路径发 RunFinished(interrupt) 后也发 Done 以便旧客户端收尾
}
```

- [ ] **Step 2: 映射到 proto**

在 `multi_turn_to_chat_event` 为新变体填 `ChatEvent` payload。

- [ ] **Step 3: 在 `run_multi_turn_stream` 开头 emit `RunStarted`**

`thread_id = session_id`，`run_id = Uuid::new_v4()`。

- [ ] **Step 4: 正常结束改为 `RunFinished{success}` + `Done`**

- [ ] **Step 5: `cargo test -p agent` / `cargo build -p backend` 通过后 Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(agent): emit run_started/run_finished and activity stream items

EOF
)"
```

---

### Task 6: `confirm` 工具 + 流内触发 interrupt

**Files:**
- Create: `crates/agent-tools/src/builtin/confirm.rs`
- Modify: `crates/agent-tools/src/builtin/clarify.rs`, `mod.rs`, `dispatch.rs`, `lib.rs`, registry
- Modify: `crates/agent-core/src/streaming.rs`（检测 HITL 工具结果 → Activity + RunFinished(interrupt) → 停止续轮）
- Modify: `crates/agent-memory/src/tools_enabled.rs` / agent toolsets（若需默认启用）

- [ ] **Step 1: `confirm` 工具**

参数：`title: String`, `body: String`。  
`dispatch` 返回 JSON：

```json
{
  "astro_hitl": true,
  "reason": "confirmation",
  "operations": [ ... from build_confirm_surface ... ],
  "response_schema": { "type":"object", "properties": { "approved": { "type":"boolean" } }, "required":["approved"] }
}
```

- [ ] **Step 2: 改造 `clarify`**

同样返回 `astro_hitl` + `build_clarify_surface` + `reason: input_required` + schema `{ "answers": object, "value": string }`。  
`clarify` 仅接受 `questions[]`，统一渲染 `ClarifyWizard`。

- [ ] **Step 3: Agent 在 `ToolResult` 后解析**

若 `result` JSON 含 `"astro_hitl": true`：

1. Emit `Activity { activity_type: "a2ui-surface", content_json: {"operations":...}, replace: true }`  
2. 构造 `Interrupt`（新 id，reason，response_schema，tool_call_id）  
3. 将 pending 存入 session 级 store（见 Task 7）  
4. Emit `RunFinished { outcome_type: "interrupt", interrupts_json }` + `Done`  
5. **不要**把 HITL JSON 再喂回模型继续多轮  

- [ ] **Step 4: 单测工具输出可被 `a2ui::validate_operations` 通过**

- [ ] **Step 5: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(tools): confirm/clarify emit A2UI HITL payloads

EOF
)"
```

---

### Task 7: Backend session pending + `ChatRequest.resume_json`

**Files:**
- Modify: `crates/agent-server/src/grpc/astro_service.rs`
- Create: `crates/agent-server/src/interrupt_store.rs`（或放 agent）

- [ ] **Step 1: 进程内 `Mutex<HashMap<session_id, InterruptPending>>`**

- Chat 开始：若 store 有 pending 且 `resume_json` 为空 → 立即 `Error` + `Done`  
- 若 `resume_json` 非空 → `apply_resume`；失败则 Error；成功则把 payload 注入为 tool results / 用户旁路消息后继续 `run_multi_turn_stream`  
- HITL 工具触发时 `insert` pending  

**注入约定（MVP）：**  
将 resume payloads 格式化为一条 user 消息，例如：`[interrupt_resume] {"interrupt_id":"...","payload":{...}}`，并在 system/tool 侧说明；或直接 `record_tool_result` 到原 tool_call_id。优先 **`ToolCallResult` 语义：对原 `tool_call_id` 写入摘要结果**（如 `approved=true`），与 AG-UI 工具绑定 interrupt 一致。

- [ ] **Step 2: 集成测或手动脚本：无 resume 被拒；满 resume 继续**

- [ ] **Step 3: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(backend): gate chat on interrupt resume payloads

EOF
)"
```

---

### Task 8: Tauri 桥接

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`（注册命令若需要）

- [ ] **Step 1: 扩展 `ChatStreamEvent`**

```rust
RunStarted { thread_id: String, run_id: String },
Activity {
    message_id: String,
    activity_type: String,
    content_json: String,
    replace: bool,
},
RunFinished {
    run_id: String,
    outcome_type: String,
    interrupts_json: String,
},
```

映射 gRPC → emit。

- [ ] **Step 2: `start_chat` 经 `StartChatRequest` 传 `resumeJson?: string` → `ChatRequest.resume_json`**

> 现行调用：`invoke("start_chat", { request: { content, provider, model, resumeJson, … } })`。  
> **不**再支持扁平顶层字段；无双路径兼容。

- [ ] **Step 3: `chat_control`：`resume` 与 `stream_resume` 均映射到流恢复；文档注释标明勿用于 interrupt**

- [ ] **Step 4: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(tauri): bridge AG-UI activity and run lifecycle events

EOF
)"
```

---

### Task 9: 前端类型 + 事件消费

**Files:**
- Modify: `apps/desktop/src/types.ts`
- Modify: `apps/desktop/src/App.tsx`

- [ ] **Step 1: 类型**

```ts
export type UiSurfaceStatus = "active" | "resolved" | "cancelled";

export type UiSurface = {
  messageId: string;
  activityType: string;
  operations: unknown[];
  status: UiSurfaceStatus;
  interrupts?: Array<{
    id: string;
    reason: string;
    responseSchema?: unknown;
  }>;
};

// ChatMessage 增加:
uiSurfaces?: UiSurface[];
```

- [ ] **Step 2: `App.tsx` listen**

- `run_started`：记录当前 `runId`  
- `activity`：parse `content_json.operations`，写入当前助手消息 `uiSurfaces`  
- `run_finished` + `interrupt`：设置 `sessionPendingInterrupts`；`streaming=false`；卡保持 `active`  
- `run_finished` + `success`：清 pending  

- [ ] **Step 3: 发送闸门**

若 `sessionPendingInterrupts.length > 0`，`onSend` 普通文本 → toast/禁用，提示先完成卡片。

- [ ] **Step 4: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(frontend): consume activity and interrupt run events

EOF
)"
```

---

### Task 10: `A2UIRenderer` + `CatalogAdapter`

**Files:**
- Create: `apps/desktop/src/a2ui/types.ts`
- Create: `apps/desktop/src/a2ui/validate.ts`
- Create: `apps/desktop/src/a2ui/CatalogAdapter.tsx`
- Create: `apps/desktop/src/a2ui/A2UIRenderer.tsx`
- Modify: `apps/desktop/src/components/ChatView.tsx`
- Modify: `apps/desktop/src/styles/chat.css`
- Modify: `apps/desktop/src/i18n/messages.ts`

- [ ] **Step 1: Adapter 支持子集组件**

`CatalogAdapter`：`Text`→`<p>`/`<h*>`，`Card`→`.a2ui-card`，`Column`/`Row`→flex，`Button`→`<button>`，`Image`→`<img>`，未知→`.a2ui-unknown`。

- [ ] **Step 2: `A2UIRenderer`**

Props：`operations`, `disabled`, `onAction(name, context)`。  
从 `createSurface`/`updateComponents` 建 id→node 树，根为 Card 或第一个无父引用节点。

- [ ] **Step 3: ChatView**

在 activities 与 reasoning 之间渲染 `message.uiSurfaces`。

- [ ] **Step 4: 样式与 i18n**

```ts
"chat.a2ui.unknown": "不支持的组件",
"chat.interrupt.pending": "请先完成上方确认或澄清",
"chat.a2ui.approve": "批准",
"chat.a2ui.deny": "拒绝",
```

- [ ] **Step 5: 接线 `onAction`**

Confirm：`approve`→`interrupt_resume` payload `{"approved":true}`；`deny`→`{"approved":false}`。  
Clarify：选项按钮 → `{"value":"..."}`。  
然后 `invoke("interrupt_resume", { sessionId, resumeJson })`（HITL 续跑；**不要**再调扁平 `start_chat`）。  
新开聊：`invoke("start_chat", { request: { …, resumeJson? } })`。

- [ ] **Step 6: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(frontend): render A2UI surfaces with catalog adapter

EOF
)"
```

---

### Task 11: `present_ui` 信息卡（无 interrupt）

**Files:**
- Create: `crates/agent-tools/src/builtin/present_ui.rs`
- Modify: registry / dispatch / agent 解析

- [ ] **Step 1: 工具参数**

`operations: Value`（数组）或 `title`+`body`+`image_url` 快捷方式。  
`dispatch`：`a2ui::validate_operations`；失败返回错误字符串。  
成功返回：

```json
{ "astro_ui": true, "operations": [ ... ] }
```

- [ ] **Step 2: Agent**

若 `astro_ui` 且非 `astro_hitl`：只 emit `Activity`，**继续**多轮（把短文本摘要写回 tool result 给模型）。

- [ ] **Step 3: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(tools): present_ui for non-blocking info cards

EOF
)"
```

---

### Task 12: 持久化（MVP 级）

**Files:**
- 定位现有 session 消息存储（`memory` sessions / Tauri history DTO）
- Modify: 序列化 `uiSurfaces` + pending interrupts 到 session 旁路文件或 DB 字段

- [ ] **Step 1: 助手消息 JSON 增加 `ui_surfaces`**

- [ ] **Step 2: pending interrupts 写入 `sessions/{id}/interrupt.json`**

重载 session 时恢复 pending + 禁用发送直至 resume。

- [ ] **Step 3: 已 resolved 卡 `status=resolved`，按钮 disabled**

- [ ] **Step 4: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(chat): persist A2UI surfaces and pending interrupts

EOF
)"
```

---

### Task 13: 回归与文档收尾

**Files:**
- Modify: `docs/superpowers/specs/2026-07-13-declarative-genui-a2ui-design.md`（Status → Implemented）
- Tests: `cargo test -p a2ui -p agent -p tools`；前端 `npm run build` 或现有 lint

- [ ] **Step 1: 跑测试清单**

```bash
cargo test -p a2ui
cargo test -p agent
cargo test -p tools
cd frontend && npm run build
```

- [ ] **Step 2: 手动验收清单（写入 PR/提交说明）**

1. 普通聊天仍流式正常  
2. confirm 卡阻塞 → 批准后续跑  
3. clarify 卡阻塞 → 选项后续跑  
4. present_ui 信息卡不阻塞  
5. pending 时无法普通发送  
6. stream pause/resume/cancel 仍可用且不回答 interrupt  

- [ ] **Step 3: 更新 spec 状态并 Commit**

```bash
git commit -m "$(cat <<'EOF'
docs: mark declarative GenUI design implemented

EOF
)"
```

---

## Spec coverage (self-review)

| Spec 要求 | Task |
|-----------|------|
| B2 gRPC/Tauri + AG-UI 语义 | 3–8 |
| ACTIVITY a2ui-surface | 5–6, 9–10 |
| 真 interrupt / resume | 4, 6–7, 9–10 |
| stream_resume 分流 | 8 |
| confirm + clarify 模板 | 2, 6 |
| 信息卡无 interrupt | 11 |
| CatalogAdapter 可替换 | 10 |
| 校验失败不渲染坏卡 | 1, 10–11 |
| 持久化 | 12 |
| 测试 / 验收 | 1–2, 4, 13 |

无 TBD 占位；`present_ui` 与 HITL 路径分离明确。
