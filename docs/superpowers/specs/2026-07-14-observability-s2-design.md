# 可观测性 S2 设计（Insights turn 折叠 + 聊天诊断）

**日期:** 2026-07-14  
**状态:** 已批准（待实现）  
**范围:** Insights 按 `turn_id` 默认折叠下钻；聊天展示当前 `turn_id`；一键复制诊断上下文  
**依赖:** [S1 可观测性对齐](./2026-07-14-observability-alignment-design.md)（已实现）  
**外部参考（仅设计借鉴）：** 参考 Agent 运行时「session = Trace、turn = 回合」产品语义（非 OTEL）

## 命名约束

- **代码、模块、类型、文件、用户可见文案、路径中不得出现 `hermes` / `Hermes` 字样。**
- 推荐命名：`turn_id`、`TurnGroup`、`copyDiagnosticContext`、Insights「回合」文案用 `turn` / 「未标注」。
- 与 S1 一致：`turn_id ===` 流式 `RunStarted.run_id`。

## 背景

S1 已贯通：

- `usage_events.turn_id`、agent 主路径（LLM + 内置工具 + MCP）写入  
- `agent.log` / `errors.log` + Prefs「日志 / 诊断」  

仍缺产品面：

1. Insights 右栏时间线仍 **平铺** `events`，不按 turn 分组  
2. `TraceEvent` / `list_trace_events` **未透传** `turn_id`  
3. 聊天仅将 `run_id` 缓存在 `currentRunIdRef`，**不展示、不复制**

## 目标与成功标准

1. Insights：选中 session 后，右栏 **默认按 turn 折叠**；展开可见 LLM→工具→费用事件链  
2. 聊天：进行中/最近一轮显示短 `turn_id`；一键复制诊断上下文（至少含 `session_id` + `turn_id`）  
3. 旧数据：无 `turn_id` 的事件归入 **「未标注」** 分组，不丢事件  

成功手测：

- 新聊两轮后，Insights 该 session 至少两个可折叠 turn 组，组头有 tokens/cost 小计  
- 聊天点击复制 → 剪贴板含当前 `session_id=` 与 `turn_id=`  
- Prefs 诊断粘贴同一 `turn_id` 仍可滤到日志（S1 契约不变）

## 明确不做（S2）

- Langfuse / OpenTelemetry 内建导出  
- `gateway.log`、`/health`、Prometheus  
- 新 Tauri「按 turn 分页」API（沿用现有 `get_trace_insights` 单次拉取）  
- 改变左栏 **session = Trace** 列表语义  
- Insights ↔ Prefs 自动跳转深链（可选后续）  
- 复制诊断时 **强制** 附带日志正文（本期默认只复制 ID；见决策）

## 决策摘要

| 项 | 选择 |
|----|------|
| 实现路线 | **扩展 `get_trace_insights` 透传 `turn_id` + 前端分组折叠** |
| Insights 交互 | 右栏 **默认折叠**；点击展开（方案 A） |
| 无 turn 旧事件 | 独立组「未标注」 |
| 聊天 turn 展示 | 复用 `RunStarted.run_id` 作为 `turn_id` UI 状态（不强制改 proto 字段名） |
| 复制诊断内容 | **最小**：`session_id` + `turn_id` 两行文本；**不做**默认附带 log 行 |
| 新 command | **不需要** |

---

## 架构

```text
UsageDb.turn_id
        │
        ▼
 list_trace_events / TraceEvent.turn_id?
        │
        ▼
 get_trace_insights ──► InsightsPanel
                        └─ chain panel: groupBy(turn_id) → 折叠组

RunStarted.run_id (= turn_id)
        │
        ▼
 App currentTurnId state ──► ChatAgentInfo 展示
                          └─ Copy diagnostic → clipboard
```

| 组件 | 职责 | 路径 |
|------|------|------|
| Usage / TraceEvent 透传 | SELECT + 序列化带 `turn_id` | `memory/src/usage/db.rs`、`trace_insights.rs` |
| FE Trace 类型 | 对齐字段 | `InsightsPanel.tsx` |
| Turn 折叠 UI | 分组、默认折叠、组头小计 | `InsightsPanel` 时间线 / 相关 CSS |
| 聊天状态 | `run_started` → `currentTurnId`；回合结束可保留「上一局」直至下一局开始 | `App.tsx` |
| 展示与复制 | 短码 + 复制按钮 | `ChatAgentInfo.tsx`（或 `ChatRightPanel`） |
| i18n | 回合 / 未标注 / 复制诊断 | `messages.ts` |

---

## 数据契约

### 后端

- `TraceEventRow` / `list_trace_events`：增加 `turn_id: Option<String>`（从 `usage_events.turn_id` 读取）  
- `TraceEvent`（serde / Tauri）：`turn_id: Option<String>`，缺省序列化为 null / omit  
- 来自 chat history 合成的 span（若有无 usage 行）：`turn_id` 可为 `None` → 进「未标注」  
- **不改** `TraceSummary` 主键（仍 `session_id`）；**不强制**在 Summary 级聚合 turns 列表（前端从 events 分组即可）

### 前端 Insights

```ts
type TraceEvent = {
  // ...existing
  turn_id?: string | null;
};

type TurnGroup = {
  turn_id: string | null; // null => 未标注
  events: TraceEvent[];
  tokens: number;
  cost_usd: number;
};
```

分组规则：

1. 稳定顺序：按组内最早事件时间（或 events 原序中首次出现）排列 turn 组  
2. 同 `turn_id` 聚为一组；`null`/空 → 「未标注」一组（可放末尾或开头，**推荐末尾**）  
3. 组头：短 turn（前 8 字符 + …）或「未标注」；`tokens` / `cost_usd` 组内求和；事件条数  
4. 默认 `expanded=false`；点击组头切换；**可选**记忆仅会话内（不必持久化 localStorage）

### 聊天

- `run_started`：`setCurrentTurnId(run_id)`（替换 ref-only）  
- `run_finished` / `done`：**保留** `currentTurnId` 供复制，直到下一轮 `run_started` 覆盖  
- 切换 session / 新会话：清空 `currentTurnId`  
- 展示：等宽短码；无 turn 时隐藏复制按钮或 disabled  
- 剪贴板格式（稳定、可被 Prefs 粘贴）：

```text
session_id=<id>
turn_id=<id>
```

---

## UI 细节

### Insights 右栏

- 保留现有事件行视觉（kind chip、tokens、I/O 截断）于展开态内  
- 折叠态仅组头 + 摘要芯片；避免重做成第二套左栏  
- 样式：沿用 `insights-trace-*`，新增 `insights-turn-group` / `insights-turn-group-head`

### ChatAgentInfo

- 在现有 usage 展示旁增加「Turn」一行 + 「复制诊断」按钮  
- 不新增独立 prefs 页；不自动跳转 Prefs

---

## 分期与测试

| 切片 | 内容 |
|------|------|
| **S2a** | `turn_id` 透出 `get_trace_insights` + 单元测试 |
| **S2b** | Insights 右栏按 turn 默认折叠 |
| **S2c** | 聊天展示 + 复制诊断 + i18n |

测试：

- Rust：`list_trace_events` 带回插入的 `turn_id`  
- FE 手测：两轮对话 → 两组折叠；旧 session 无 turn → 仅「未标注」或混合  
- 复制格式冒烟：粘贴到 Prefs turn 过滤可用（人工）

---

## 风险

| 风险 | 缓解 |
|------|------|
| chat-history 合成事件无 turn | 「未标注」桶；不阻塞分组 |
| 超长 session 事件多 | 仍受 `TRACE_EVENTS_LIMIT`；不新开分页 API |
| `run_id` 与用户心智「turn」 | UI 文案用「回合 / Turn」；复制键名仍 `turn_id=` |

## 与 S1 / S3 关系

- 依赖 S1 写入路径；Prefs 过滤契约不变  
- S3（OTEL / log index）仍不动  
- 原 S2 清单中 Langfuse、gateway.log、health **顺延** 未排期项
