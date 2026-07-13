# Insights 协作 2D 设计（Orchestration 可视化竖切）

**日期:** 2026-07-13  
**状态:** 已批准 / 已实现  
**范围:** 洞察面板「协作」Tab：近期编排列表 + 步骤条 + SVG 聚合图  
**关联:**  
- `docs/superpowers/specs/2026-07-13-multi-agent-orchestration-design.md`  
- `docs/superpowers/specs/2026-07-13-usage-insights-design.md`

## 目标

在现有 Insights「用量」能力旁增加 **协作** 视图，让用户能：

1. 看到近期 `orchestration_run` 的结果与步骤推进；  
2. 在同一时间窗内看到 Agent / 临时角色之间的 handoff 聚合关系。

不引入 3D、外部图库或独立导航。

## 背景与约束

- 编排状态权威在 `~/.astro/orchestration.db`。  
- Handoff 遥测在 `usage_events`（`kind=orchestration`，`meta_json` 含 `from`/`to`/`phase`/`orchestration_id` 等）；**不计入**用量 KPI `calls`。  
- 洞察面板已有 period（月/季/年）与 Agent 筛选；协作 Tab 复用。  
- 产品为单机 Tauri；图用纯 SVG。

## 方案选择（已定）

| 决策 | 选择 |
|------|------|
| MVP 形态 | 薄组合：列表 + 步骤条 + 小聚合图 |
| 入口 | 洞察面板 Tab「用量 \| 协作」 |
| 数据 | 列表/步骤 ← `orchestration.db`；图 ← `usage_events` |
| API | 单一命令 `get_collaboration_insights` |
| 图渲染 | 手写 SVG（边粗细 ∝ weight） |

## 数据与 API

### 命令

`get_collaboration_insights`

**参数（与用量洞察对齐）：**

| 字段 | 说明 |
|------|------|
| `period` | `month` \| `quarter` \| `year` |
| `agent_id` | 可选；过滤编排的 `parent_agent_id`，以及图边涉及的节点（见下） |
| `as_of` | 可选 RFC3339；锚定 period 窗口 |

**返回：**

```json
{
  "orchestrations": [
    {
      "id": "uuid",
      "goal": "string",
      "status": "queued|running|done|failed|cancelled",
      "parent_agent_id": "string",
      "session_id": "string|null",
      "created_at": "RFC3339",
      "updated_at": "RFC3339",
      "finished_at": "RFC3339|null",
      "error": "string|null",
      "result_summary": "string|null",
      "steps": [
        {
          "seq": 0,
          "role": "string",
          "agent_id": "string|null",
          "status": "pending|running|done|failed|skipped",
          "output": "string|null",
          "error": "string|null"
        }
      ]
    }
  ],
  "graph": {
    "nodes": [
      { "id": "workspace", "label": "workspace", "kind": "agent" }
    ],
    "edges": [
      { "from": "workspace", "to": "role:researcher", "weight": 3 }
    ]
  }
}
```

### 列表查询规则

- 源：`orchestration.db` 的 `orchestrations` + `orchestration_steps`（按 `seq`）。  
- 时间窗：与 `usage_db` 相同的 period 半开区间 `[start, end)`，用 `created_at` 过滤。  
- `agent_id` 若设：`parent_agent_id = agent_id`。  
- 排序：`created_at DESC`；默认上限 **50** 条。  
- 步骤 `output` 在 API 层再截断至 **2KB**（UTF-8 安全），避免 UI payload 过大。

### 图聚合规则

- 源：`usage.db` / `usage_events`，`kind = 'orchestration'`。  
- 只统计 `meta_json.phase = 'end'` 的事件（一次 step 完成/失败计 1）。  
- `from` / `to` 取自 `meta_json`；节点 `id` 即该字符串。  
  - 已有 Agent：`agent_id`  
  - 临时角色：`role:{role}`（与写入遥测时一致）  
- `weight` = 边出现次数。  
- **Agent 过滤（写死）：** `agent_id` 有值时，仅保留 `from = agent_id OR to = agent_id` 的边；节点为这些边的端点并集。无 `agent_id` 时用时间窗内全部边。  
- `kind`：`id` 以 `role:` 开头 → `role`，否则 → `agent`。  
- **不**把这些事件计入 Insights 用量 `calls` KPI（保持现状）。

## UI

### 结构

- `InsightsPanel` 增加视图切换：**用量 | 协作**（i18n）。  
- 协作视图复用顶部 period tabs 与 AgentPicker。  
- 布局（桌面）：左约 55% 列表+步骤，右约 45% 图；窄屏上下堆叠。

### 列表与步骤

- 每行：`goal`（截断）、`status` 色点、步骤数或简短角色链。  
- 选中后展示步骤条：`seq` · `role` · `status`；可展开看截断 `output` / `error`。  
- `failed` 时高亮失败 step 与 orchestration `error`。

### 图

- SVG：圆形节点 + 直线边；`stroke-width` 随 weight 映射（设 min/max）。  
- 边旁或 tooltip 显示 weight。  
- 布局：简单分层或圆形排布即可（按节点数），**不**引入力导向库。  
- 空图与空列表统一空态文案（引导 `orchestration_run`）。

### 状态

- 加载 / 错误样式对齐现有 Insights。  
- Tab 切换时：协作 Tab 调用 `get_collaboration_insights`；用量 Tab 仍用 `get_usage_insights`（可缓存上次结果，非必须）。

## 模块落点

| 位置 | 职责 |
|------|------|
| `memory` | 编排列表查询（可放 `orchestration_db` 扩展）+ 图聚合（`usage_db` 或小模块 `collab_insights`） |
| Tauri / backend 命令 | `get_collaboration_insights` 接线 |
| `InsightsPanel.tsx` + CSS + i18n | Tab、列表、步骤条、SVG 图 |

## 验收

1. 无编排数据时协作 Tab 空态清晰，不报错。  
2. 跑过 `orchestration_run` 后，列表出现对应行，步骤状态与 DB 一致。  
3. 同窗内图上出现 parent → role/agent 边，weight ≥ 1（有 `phase=end` 遥测时）。  
4. 切换 period / Agent 后列表与图一起刷新。  
5. 用量 Tab KPI `calls` 不因 orchestration 事件增加。  
6. 旧用量图表与排行行为不变。

## 非目标

- 3D、WebGL、D3/力导向物理、可拖拽改图  
- 编排取消 UX、并行/DAG、重启续跑  
- 独立侧栏「协作」导航  
- 自定义定价 / CSV / 预算（属用量后续）  
- 历史 JSON 编排回填  

## 测试要点

- memory：period 窗过滤编排；agent 过滤；output 截断；图只计 `phase=end`；agent 边过滤  
- 前端：Tab 切换、空态、选中步骤条（可用 mock invoke）  
- 回归：`get_usage_insights` / orchestration_db 既有测试仍绿  

## 后续（本竖切之后）

- 点击图节点反选/过滤列表  
- 编排详情抽屉 / Tauri 专用协作面板  
- 更优图布局或轻量交互  
