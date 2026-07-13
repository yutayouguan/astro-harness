# 聊天气泡时间线交错（Thinking / Tools / A2UI）

**日期:** 2026-07-13  
**状态:** 已批准 / 已实现  
**范围:** 助手气泡内按事件顺序交错渲染「思考 / 工具活动 / A2UI」；最终正文仍在底部；流式与 SessionStore 重进一致  
**非目标:** 工具前中间正文交错（P1）；Hermes 品牌命名；Gateway / compaction  

**关联:**  
- `docs/superpowers/specs/2026-07-13-session-store-design.md`  
- `docs/superpowers/specs/2026-07-13-declarative-genui-a2ui-design.md`  
- 外部参考（仅行为对照）：[Hermes TUI 分组展示](https://hermes-agent.nousresearch.com/docs/user-guide/tui)、[交错需求 #18241](https://github.com/NousResearch/hermes-agent/issues/18241)（Hermes **尚未**落地真交错）

## 背景与动机

Astro 当前把助手轮次收成「整段 `reasoning` + `activities[]` + `uiSurfaces[]` + `content`」，渲染曾按 GenUI 规格把工具放在思考之上，后改为「思考→工具→卡→正文」分组顺序。  
对人来说，多步回合的因果链是：**想一段 → 调工具 → 再想 → 出卡 → … → 最终回答**。分组展示会把因果打散。

Hermes 持久化与我们类似（`reasoning` 整段 + `tool_calls`），TUI **按类型分块**，并非时间线交错。我们要在兼容该存储形状的前提下，**额外持久化 timeline**，UI 做到真交错。

## 决策摘要

| 项 | 选择 |
|----|------|
| 交错内容 | 思考 + 工具活动 + A2UI surface；**正文固定在气泡底部** |
| 持久化 | 完整：流式、localStorage、`SessionStore` 重进一致 |
| 实现路径 | **A**：前端 `segments[]` + DB 旁路 JSON（复用/嵌套 `reasoning_details`） |
| 旧数据 | 无 timeline 时回退分组顺序：思考 → 工具 → A2UI → 正文 |
| 中间正文 | 本期不做（工具调用前的 interim assistant text 仍并入最终 `content` 或丢弃策略与现流式一致） |

## 数据模型

### 前端 `ChatMessage`

保留现有字段（兼容）：`reasoning`、`activities`、`uiSurfaces`、`content`。

新增：

```ts
type ChatTimelineSegment =
  | {
      type: "reasoning";
      id: string;
      text: string;
      at: number;
      durationSec?: number;
    }
  | {
      type: "activity";
      id: string; // 与 ChatActivity.id 对齐，便于原地更新
      at: number;
    }
  | {
      type: "surface";
      id: string; // 与 UiSurface.messageId 对齐
      at: number;
    };

// ChatMessage 增加：
segments?: ChatTimelineSegment[];
```

约定：

- `activity` / `surface` 段只存 **引用 id**；实体仍在 `activities` / `uiSurfaces`（单一数据源，避免双写漂移）。  
- `reasoning` 段内联 `text`（同一段可流式追加）；整轮结束后可用 `reasoning` 字段存「拼接全文」供旧路径/搜索兼容。  
- 渲染：若 `segments?.length`，按 segments 顺序画；否则回退分组布局。

### SessionStore

不新增表。在 assistant 消息的 `reasoning_details`（已有 JSON 列）中写入：

```json
{
  "astro_timeline_v1": [
    { "type": "reasoning", "id": "r1", "text": "...", "at": 1710000000123 },
    { "type": "activity", "id": "call_abc", "at": 1710000000456 },
    { "type": "surface", "id": "a2ui-surface-call_abc", "at": 1710000000500 }
  ]
}
```

规则：

- 读写时 **合并**：保留 `reasoning_details` 上其它键（若将来有 provider opaque 字段），只维护 `astro_timeline_v1`。  
- `tool_calls` / `reasoning` / `content` 仍按现逻辑写入，保证 API 回放与 FTS 不回归。  
- `build_chat_history`：从 `reasoning_details.astro_timeline_v1` 还原 `segments`，并照常折叠 `activities`；`get_chat_history` DTO 增加可选 `segments`。

localStorage（`chatSessionStore`）直接序列化带 `segments` 的 `ChatMessage`。

## 流式拼装（App.tsx）

对当前助手消息维护 `segments`：

| 事件 | 行为 |
|------|------|
| `reasoning` token | 若末段为 `reasoning` → 追加 `text`；否则 `push` 新 reasoning 段 |
| `tool_call_delta` / 首次出现工具 | 若末段是 reasoning → **封口**该段；确保 `activities` 有条目；`push` activity 段（同 id 不重复 push，只更新 activity） |
| `tool_call` 完成 | 更新对应 `activities[id]`；不新增段 |
| `activity`（A2UI） | 更新/插入 `uiSurfaces`；`push` surface 段（replace 同 messageId 时更新实体，段保留） |
| `token`（正文） | 仍写入 `content`（底部），**不**进 segments |
| `done` / interrupt | 结算末段 reasoning 的 `durationSec`；落盘时带上完整 `astro_timeline_v1` |

多轮工具：每一轮「再思考」自然成为新的 reasoning 段，插在两次 activity 之间。

## 渲染（ChatView）

有 `segments` 时：

1. 按顺序渲染：  
   - `reasoning` → `MsgReasoning`（流式末段可 `active`）  
   - `activity` → 单条活动（或复用 `MsgActivity`，外包一层；父级 `ActivityGroup` 可选：有 timeline 时 **取消父级整包折叠**，或父级仅作「本轮工具」汇总且默认展开——**本期：timeline 模式下不使用父级整包，逐段展示**）  
   - `surface` → `A2UIRenderer`  
2. 然后时间戳（若开）→ `content` markdown → token 角标。

无 `segments`：保持「思考 → 工具组 → A2UI → 正文」回退。

## Agent / 落盘

- streaming 结束写入 assistant 行时：传入 `reasoning_details` 含 `astro_timeline_v1`。  
- 若时间线仅在前端拼装：Tauri 在 `done` 前不持有 segments——则以 **前端权威** 为准时，需么（a）前端通过已有 history 写回 API（若无则不做），么（b）**agent 侧同样维护 timeline** 并写入 DB。  

**本期选择（b）：** agent `streaming` 在解析 reasoning / tool / astro activity 时同步维护 `Vec<TimelineSegment>`，随 `append_message` 写入；前端流式自建 segments 用于即时 UI，restore 以 DB 为准。两边结构同构（JSON schema 一致）。

## 兼容与迁移

- 旧消息无 `astro_timeline_v1`：前端回退分组；不强制 backfill。  
- 可选后续：用 `reasoning` 全文 + `activities` 按 `at` 粗排生成伪 timeline（不保证准确），**本期不做**。

## 测试

- 前端纯函数：给定事件序列 → 期望 `segments`（node:test）。  
- memory：`append_message` 写入带 `astro_timeline_v1` 的 `reasoning_details`，`build_chat_history` 读出 `segments`。  
- agent（可选 ScriptedProvider）：一轮 reasoning→tool→reasoning→done，断言 DB JSON 段顺序。  
- 手工：DeepSeek 思考模型多工具回合；重进会话顺序不变；A2UI confirm 卡插在对应工具后。

## 验收标准

1. 流式过程中可见「思考块 ↔ 工具行 ↔ A2UI」按发生顺序交错。  
2. 最终助手正文仍在气泡最下方。  
3. 清 localStorage 后从 DB 恢复，交错顺序与结束时一致。  
4. 无 timeline 的旧消息仍可读（分组回退）。  
5. 代码/文案不含 `hermes` 字样（spec 外链除外）。

## 实现顺序建议

1. 共享 JSON 形状 + 前端类型 / 纯函数拼装 + ChatView 渲染  
2. App 流式接线  
3. agent streaming 维护 timeline 并写入 `reasoning_details`  
4. SessionStore history DTO + 前端 restore  
5. 测试与手工验收  

## 分期（明确不做）

| 项 | 何时 |
|----|------|
| 工具前 interim 正文进 timeline | P1 |
| 父级 ActivityGroup 与 timeline 混排策略细化 | 可随视觉反馈微调 |
| 旧消息启发式 backfill | 可选，非必需 |
