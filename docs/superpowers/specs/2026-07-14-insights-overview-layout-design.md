# 数据洞察总览布局设计

**日期:** 2026-07-14  
**状态:** 已批准 / 已实现  
**范围:** Insights 信息架构、首屏布局与子页摘要指标
**前置:**  
- `docs/superpowers/specs/2026-07-13-usage-insights-design.md`  
- `docs/superpowers/specs/2026-07-13-usage-insights-polish-design.md`  
- `docs/superpowers/specs/2026-07-13-insights-collaboration-2d-design.md`  
- 横滑整页错位修复（`insights.css` overflow-x 锁定）

## 目标

打开「数据洞察」时，用户先看到**全局结论**（花了多少、趋势如何、钱主要去哪），再按需下钻到模型 / 工具 / Tracing。收敛当前「多 KPI + 重复排行」的噪声，让首屏回答统计分析的首要问题。

## 用户决策摘要

| 决策项 | 选择 |
|--------|------|
| 首要任务 | 都要，但首屏必须有总览 |
| 总览主角 | 核心数字 + 趋势图 + 花钱排行；异常轻量出现；活跃摘要不当首屏主角 |
| 导航 | 总览为默认首页；四 Tab 平级 |
| 总览排版 | 经典分析台：3 KPI → 左大趋势 + 右花钱 Top |

## 信息架构

默认进入 **总览**。顶栏控件不变并作用于所有 Tab：

- Agent 筛选（全部 / 单个）
- 周期：`days30` | `days90` | `days365`

| Tab | 职责 |
|-----|------|
| 总览 | 本期费用 / Tokens / 调用；用量趋势；厂商费用 Top |
| 模型用量 | 厂商 / 模型 / Agent 明细；模型 Tokens 横条；精简次要 KPI |
| 工具技能 | 工具 / Skills / MCP / 定时（结构基本保持） |
| Tracing | 会话调用链（结构基本保持） |

**下钻：** 总览上的「更多 / 查看明细」可切换到「模型用量」或「工具技能」。本期不做深链滚动定位或带 query 的子状态。

## 总览页布局

自上而下：

1. **异常条（条件渲染）**  
   - 例：未计价模型提示  
   - 无事则不占位

2. **KPI 一行（恰 3 项）**  
   - 费用（估）  
   - Tokens  
   - 调用合计  
   - 不展示：模型数、厂商数等次要计数（留给模型 Tab）

3. **主区网格 ≈ 1.5fr : 1fr**  
   - **左：用量趋势** — 大图；指标切换 calls / tokens / cost（沿用现有）  
   - **右：花钱 Top** — 默认按**厂商** `cost_usd` Top 5；条形占比 + 金额；「更多」→ 模型用量 Tab

**窄屏（≲900px）：** KPI 可折行；主区上下堆叠（趋势在上、排行在下）。

**横滑：** 面板继续 `overflow-x: hidden`；仅图表容器可横向滚动并 `overscroll-behavior-x: contain`。

## 模型用量 Tab

相对现状减少与总览的重复（以下为明确取舍，不再「或」）：

- **去掉**现有 6 枚 KPI 与整宽用量趋势（由总览承载）
- 顶部使用 **4 个摘要 KPI**：总 Token、费用估算、模型数、主力模型；副文案补充模型调用数、计价状态、活跃 Agent 和主力模型调用量。
- **主内容：** 厂商 / 模型 / Agent 三列排行 + 模型 Tokens 横条
- **未计价提示只出现在总览**；模型 Tab 不再重复展示同一条

## 工具 / Tracing

- 工具技能页顶部使用 4 个摘要 KPI：能力总调用、内置工具、Skills、连接与自动化（MCP + Cron）；副文案显示能力覆盖数、分类占比和 MCP/Cron 拆分。
- Tracing 页顶部使用 4 个摘要 KPI：追踪记录、事件、平均耗时、Agent 动作；副文案显示 Trace Token/费用、每条 Trace 平均事件数、LLM/异常事件数以及工具/技能拆分。
- 各摘要卡使用独立语义图标与克制色调，但继续复用同一玻璃材质、响应式网格、降低透明度与高对比模式。

## 数据与 API

- **复用**现有 `get_usage_insights` 与 `get_trace_insights`
- KPI / series / rankings 均从现有响应派生；厂商维度沿用当前前端聚合逻辑
- `TraceKpis` 新增 `tokens`、`cost_usd`、`avg_duration_ms`、`error_events`：聚合当前筛选范围内返回的 Trace；平均耗时只统计至少包含两个事件且时间戳可解析的调用链；异常数只统计明确标记为 `error` / `failed` 的事件。
- 暂不展示“工具成功率 / P95 工具耗时”：`usage_events` 尚未完整持久化工具状态和耗时，将未知状态当作成功会产生误导。

## 前端落点

- `apps/desktop/src/components/settings/InsightsPanel.tsx` — 四 Tab 渲染、子页摘要计算与 Trace KPI 消费
- `apps/desktop/src/styles/features/insights.css` — 总览与子页 KPI 网格、语义色调、响应式与可访问性回退
- `apps/desktop/src/i18n/messages.ts` — `insights.view.overview` 等中英键
- `crates/agent-usage/src/trace_insights.rs` — Trace Token、费用、平均耗时与异常事件聚合

## 验收

1. 进入洞察默认落在「总览」  
2. 有数据时，首屏约一屏内可见：核心 KPI + 趋势 + 花钱 Top（宽屏）
3. 切换周期 / Agent 后总览与其它 Tab 数据一致过滤  
4. 「更多」能切到模型用量 Tab  
5. 触控板横向滑动不再拖偏整页  
6. 模型 / 工具 / Tracing 子页均有 4 张摘要卡，空数据真实显示 0 或占位，不编造趋势
7. 工具 / Tracing 明细功能回归不破

## 非目标（本期不做）

- 预算告警、CSV 导出、自定义单价  
- 首屏塞 Tracing 摘要
- 新查询命令或预聚合表
- 深链（滚动到指定排行行、URL 状态）  
- 用图表库替换纯 CSS/SVG 柱图  

## 测试要点

- 默认 `view === overview`（或产品等价默认）  
- 总览与子页摘要字段不重复堆叠；空 series / 空 rankings 空态合理
- 窄屏断点网格变为单列  
- i18n 中英键齐全  
