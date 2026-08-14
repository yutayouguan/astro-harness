# 洞察面板（Usage Insights）设计

**日期:** 2026-07-13  
**状态:** 已批准 / 已实现  
**范围:** 侧边栏统计分析可视化 MVP（SQLite 事件存储）  
**费用更新:** LLM 费用估算已改为路由感知定价（官方快照 / models API），不再使用 LiteLLM 公开价目。

## 目标

在左侧导航新增「洞察」入口，统一展示工具 / 技能 / MCP / 定时任务调用次数，以及 LLM Token 与金钱消耗（估算），支持年 / 季 / 月切换与按 Agent 筛选。采用 SQLite 事件表 + 查询聚合，为后续多 Agent 协作可视化预留扩展。

## 背景与约束

- 现有 `usage-stats.json` 仅有按 Agent 的**累计**工具集 / 技能次数，无时间桶。
- `tool-calls.jsonl` 有时间戳但未做结构化聚合；会话消息含 token usage，未做成本汇总。
- Tauri 侧已缓存 LiteLLM `model_prices_and_context_window.json`。
- 多 Agent 运行时遥测尚薄；3D / 协作图明确后置。
- 用户决策：**只记今后数据，不回填历史**；费用用 **LiteLLM 公开价估算**。

## 方案选择

在「侧边栏 + 按日桶 JSON」「SQLite 事件表 + 查询 API」「仅增强现有 Tools/Skills 面板」三者中，采用 **SQLite 事件表 + 查询 API**，便于统一维度查询与后续扩展。

## 信息架构

- 侧边栏新项：`insights`（文案「洞察」），位置靠近 Tools / Cron。
- 页内控件：`period` ∈ `month` | `quarter` | `year`；可选 Agent 筛选（全部 / 单个）。
- KPI：调用合计、Tokens、费用（估）、活跃 Agent 数。
- 趋势：按时间桶的调用 / tokens / cost 序列。
- 排行：统一维度（tool / skill / mcp / cron）+ 按 Agent / 按模型（llm）拆分。
- Tools / Skills 面板现有累计 chip **保留**，不迁移到洞察。

## 数据模型

独立库路径：`~/.astro/usage.db`（rusqlite，风格对齐 `artifacts.db` / session DB）。

### 表 `usage_events`

| 列 | 说明 |
|---|---|
| `id` TEXT PK | UUID |
| `ts` TEXT NOT NULL | ISO8601（UTC） |
| `kind` TEXT NOT NULL | `tool` \| `skill` \| `mcp` \| `cron` \| `llm` |
| `name` TEXT NOT NULL | 工具集 id / skill_id / MCP 名 / cron job / model id |
| `agent_id` TEXT NOT NULL | |
| `session_id` TEXT | 可选 |
| `prompt_tokens` INTEGER DEFAULT 0 | 主要用于 `llm` |
| `completion_tokens` INTEGER DEFAULT 0 | |
| `total_tokens` INTEGER DEFAULT 0 | |
| `cost_usd` REAL DEFAULT 0 | **写入时**按 LiteLLM 价估算 |
| `meta_json` TEXT | 可选：原工具名、provider 等 |

索引：`(ts)`、`(agent_id, ts)`、`(kind, name, ts)`。

不做预聚合表；年 / 季 / 月通过查询时间窗 + `strftime` 分桶实现。

### 与旧数据

- 保留 `usage-stats.json`；新调用可**双写**（bump JSON + insert event）。
- **不**扫描历史 JSONL / 消息做回填。

## 写入路径

失败可忽略，不阻塞主流程（与现有审计日志一致）。

| kind | 触发点 |
|---|---|
| `tool` / `skill` | `tools` dispatch 与 agent loop 中现有 `record_usage_tool_call` 旁路：每次工具调用写一条 `kind=tool`（`name`=工具集 id）；若为 `skills` 且 `skill_id` 非空，再额外写一条 `kind=skill`（`name`=skill_id） |
| `mcp` | MCP 工具执行完成时 |
| `cron` | cron runner 一次 run 完成时 |
| `llm` | 流式结束收到 FinalUsage / chat usage 时；用 model id 查价写入 `cost_usd` |

未知模型：`cost_usd = 0`；前端可提示「部分未计价」。币种 USD，UI 标明「估」。

费用查询复用已有 LiteLLM 价目缓存：抽共享 pricing 函数，供写入方在 insert 前算好 cost（或由 memory 接受单价参数）。

## 查询 API

Tauri 命令（名称可微调，语义如下）：

```text
get_usage_insights({
  period: "month" | "quarter" | "year",
  as_of?: string,       // 默认 now
  agent_id?: string | null
}) → {
  kpis: { calls, tokens, cost_usd, active_agents },
  series: [{ bucket, calls, tokens, cost_usd }],
  rankings: {
    by_kind: [{ kind, name, calls, tokens?, cost_usd? }],
    by_agent: [...],
    by_model: [...]
  }
}
```

## 前端

- `InsightsPanel.tsx` + `insights.css`；`App.tsx` NAV / i18n / 图标。
- 轻量 2D 图表（如 uPlot / Chart.js 或纯 CSS 柱状）；**不**引入 3D 依赖。
- 空态：说明「上线后开始累计」。

## 模块落点

- `crates/agent-memory/src/usage_db.rs`：建库、insert、聚合查询。
- 写入钩子：dispatch / agent loop / MCP / cron / streaming usage。
- Tauri：`get_usage_insights` 注册与前端 invoke。

## MVP 验收

1. 侧边栏可进入「洞察」；月 / 季 / 年切换改变时间窗与序列。
2. 新产生的 tool / skill / mcp / cron / llm 事件出现在 KPI 与排行。
3. 费用为 LiteLLM 估算并标注。
4. 不回填历史；旧累计 JSON 仍可用。

## 非目标（二期+）

- 多 Agent 协作 2D 关系图 / 时间线
- 3D 可视化
- 用户自定义单价覆盖、CSV 导出、预算告警
- 历史回填

## 测试要点

- `usage_db`：insert 后按 period / agent 过滤聚合正确。
- kind 映射（尤其 skill_id、mcp 名）单测。
- 未知模型 cost=0；已知模型 cost 与价目表公式一致。
- 前端：空库空态；有 fixture 数据时 KPI / 排行渲染。
