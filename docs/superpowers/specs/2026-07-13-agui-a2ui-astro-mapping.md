# Astro ↔ AG-UI ↔ A2UI 映射表

> 对照日期：2026-07-13  
> 依据：[AG-UI Events](https://docs.ag-ui.com/sdk/js/core/events)、[AG-UI Interrupts](https://docs.ag-ui.com/concepts/interrupts)、[A2UI v0.9 Protocol](https://github.com/google/A2UI/blob/main/specification/v0_9/docs/a2ui_protocol.md)、[basic catalog](https://github.com/google/A2UI/blob/main/specification/v0_9_1/catalogs/basic/catalog.json)、[AG-UI a2ui-middleware](https://github.com/ag-ui-protocol/ag-ui/tree/main/middlewares/a2ui-middleware)  
> Astro 现状：`gRPC ChatEvent` → Tauri `chat-stream-{sid}` → React；控制面 `ChatControl(pause|resume|cancel)`。

---

## 1. 分层关系（一句话）

| 层 | 协议 | 在 Astro 中的角色 |
|----|------|------------------|
| 交互运行时 | **AG-UI** | 事件语义：run 生命周期、文本/工具流、状态、HITL interrupt、Activity |
| GenUI 载荷 | **A2UI v0.9** | Surface 声明式 JSON；经 AG-UI **Activity** 承载（生态惯例：`activityType: "a2ui-surface"`） |
| 传输 | Astro **gRPC + Tauri** | 不采用 AG-UI 默认 HTTP/SSE；需语义对齐，而非换传输 |

官方惯例：A2UI 操作列表放进 `ACTIVITY_SNAPSHOT.content.operations`；用户点击走 A2UI `action`，HITL 暂停走 AG-UI `RunFinished.outcome.interrupt`。

---

## 2. AG-UI 事件 → Astro 现状

| AG-UI Event | Astro 今日 | 映射建议 | 优先级 |
|-------------|------------|----------|--------|
| `RUN_STARTED` | 隐式（开始 Chat） | 显式 `run_started{threadId=sessionId, runId}` | P1 |
| `RUN_FINISHED` | `done` | `done` 扩展可选 `outcome`（success / interrupt） | P0（HITL） |
| `RUN_ERROR` | `error` | 对齐；可加 `code` | P1 |
| `STEP_*` | 无 | 可映射到活动卡步骤；首期可跳过 | P3 |
| `TEXT_MESSAGE_START/CONTENT/END` | 连续 `token` | 保持 `token`；适配层可合成 triad | P2 |
| `TEXT_MESSAGE_CHUNK` | — | 客户端便捷；不必进 proto | — |
| `TOOL_CALL_START/ARGS/END` | `tool_call_delta` | 语义已接近；可补 start/end 边界 | P2 |
| `TOOL_CALL_RESULT` | `tool_call`（含 result） | 拆名或文档对齐即可 | P2 |
| `REASONING_*` | `reasoning` 增量 | 同 token：适配层合成 | P2 |
| `STATE_SNAPSHOT/DELTA` | 无 | 首期不做共享状态 | P3 |
| `MESSAGES_SNAPSHOT` | 历史另 RPC | 保持独立；interrupt 边界前可按需发 | P3 |
| `ACTIVITY_SNAPSHOT/DELTA` | `ChatActivity`（扁平文本） | **新增** `activity` 事件；A2UI 走此通道 | **P0** |
| `CUSTOM` / `RAW` | 无 | 备用扩展；A2UI 优先用 Activity 而非 Custom | P3 |

### Interrupt（HITL）↔ Astro 控制面

| AG-UI | Astro 今日 | 差异 | 映射建议 |
|-------|------------|------|----------|
| `RunFinished { outcome: interrupt, interrupts[] }` | 无；流直接 `done` | Astro 无「等用户再开新 run」模型 | 新增 interrupt 结局；会话挂起待 `resume` |
| `RunAgentInput.resume[{interruptId, status, payload}]` | `ChatControl.resume` = **恢复流式生成** | **语义冲突**：同名不同义 | 控制动作改名或分流：`stream_resume` vs `interrupt_resume` |
| `reason: tool_call \| input_required \| confirmation` | `clarify` / `confirm` 通过 A2UI HITL surface 挂起 run | 已结构化、同回合阻塞 | clarify/confirm → interrupt + A2UI 表单 |
| `responseSchema` | 无 | — | 与 A2UI dataModel / 表单提交对齐 |
| `pause` / `cancel` | `ChatControl.pause/cancel` | AG-UI 无对等 pause | **保留** Astro 流控，不塞进 AG-UI interrupt |

---

## 3. A2UI v0.9 → Astro 载荷

### Server → Client 消息

| A2UI 消息 | 含义 | Astro 承载 |
|-----------|------|------------|
| `createSurface` | 建 surface + catalogId | `ACTIVITY_SNAPSHOT`（`a2ui-surface`）的 operations[] 之一 |
| `updateComponents` | 扁平组件列表（id 引用） | 同上，可流式多次 snapshot/delta |
| `updateDataModel` | path 替换数据 | 同上 |
| `deleteSurface` | 移除 UI | 同上或本地清理 |

每条 envelope 含 `version: "v0.9"`（或 v0.9.1）。

### Client → Server

| A2UI 消息 | 含义 | Astro 承载 |
|-----------|------|------------|
| `action` | 按钮等交互：`name, surfaceId, sourceComponentId, timestamp, context` | 新 RPC / Chat 上行：`ui_action`（可附 `a2uiClientDataModel`） |
| `error` | 校验失败等 | 日志 + 可选回传 agent |

### Basic Catalog（首期建议子集）

| 组件 | 澄清提问 | 确认/授权 | 信息/天气卡 |
|------|:--------:|:---------:|:-----------:|
| Text, Icon, Divider | ✓ | ✓ | ✓ |
| Card, Column, Row | ✓ | ✓ | ✓ |
| Button | ✓ | ✓ | 可选 |
| TextField, ChoicePicker, CheckBox | ✓ | 可选 | — |
| DateTimeInput, Slider | 按需 | — | — |
| Image, List, Tabs | — | — | ✓ |
| Modal, Video, AudioPlayer | 首期不做 | — | — |

校验/格式化函数（`required`, `regex`, `formatString`…）随 renderer 逐步支持。

---

## 4. 端到端目标数据流（推荐语义）

```
Agent 需要 UI
  → 产出 A2UI JSONL（createSurface / updateComponents / updateDataModel）
  → 适配层打包为 AG-UI ACTIVITY_SNAPSHOT
       { activityType: "a2ui-surface", content: { operations: [...] } }
  → Astro ChatEvent.activity → Tauri → ChatMessage.uiSurfaces
  → React A2UI Renderer（basic catalog）

用户点击确认 / 提交澄清
  → A2UI action (+ 可选 dataModel)
  → 若需阻塞 Agent：AG-UI interrupt resume
  → 否则：作为 tool result / 普通用户消息继续
```

---

## 5. 缺口清单（相对「声明式 GenUI」）

| 缺口 | 说明 |
|------|------|
| 无 Activity 结构化通道 | 今日只有文本 `ChatActivity` |
| 无 interrupt 结局 | `done` 无法表达「等用户」 |
| `resume` 命名冲突 | 流恢复 ≠ interrupt 恢复 |
| 无 A2UI renderer / catalog | 前端未接组件目录 |
| 无 ui_action 上行 | ChatControl 仅 pause/resume/cancel |
| clarify GenUI | `ClarifyWizard` A2UI Surface + interrupt |

---

## 6. 对接入方案的含义

| 方案 | 与本表关系 |
|------|------------|
| **A. 协议适配层** | 在现有 gRPC/Tauri 上增加 `activity` + `ui_action` + interrupt 语义；事件名/字段对齐上表；**不**引入完整 AG-UI HTTP 服务端。最贴合本表。 |
| **B. 完整 AG-UI 运行时** | 直接实现/接入 AG-UI 事件流与客户端；A2UI middleware 可复用；与 Tauri 摩擦大。 |
| **C. 仅 A2UI** | 跳过 Activity/Interrupt，工具结果塞 JSON；映射表中 HITL/生命周期仍要自造，长期更贵。 |

**建议：选 A**，首期只落地 P0：`ACTIVITY_*` 承载 A2UI + interrupt/`ui_action`；文本/工具/reasoning 保持现有事件，由文档声明「语义等价于 AG-UI xxx」。
