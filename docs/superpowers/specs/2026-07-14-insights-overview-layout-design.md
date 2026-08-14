# 数据洞察总览布局设计

**日期:** 2026-07-14  
**状态:** 已批准 / 已实现  
**范围:** Insights 信息架构与首屏布局；前端展示层  
**前置:**  
- `docs/superpowers/specs/2026-07-13-usage-insights-design.md`  
- `docs/superpowers/specs/2026-07-13-usage-insights-polish-design.md`  
- `docs/superpowers/specs/2026-07-13-insights-collaboration-2d-design.md`  
- 横滑整页错位修复（`insights.css` overflow-x 锁定）

## 目标

打开「数据洞察」时，用户先看到**全局结论**（花了多少、趋势如何、钱主要去哪），再按需下钻到模型 / 工具 / 协作 / Tracing。收敛当前「多 KPI + 重复排行」的噪声，让首屏回答统计分析的首要问题。

## 用户决策摘要

| 决策项 | 选择 |
|--------|------|
| 首要任务 | 都要，但首屏必须有总览 |
| 总览主角 | 核心数字 + 趋势图 + 花钱排行；异常轻量出现；活跃摘要不当首屏主角 |
| 导航 | 总览为默认首页；五 Tab 平级 |
| 总览排版 | 经典分析台：3 KPI → 左大趋势 + 右花钱 Top |

## 信息架构

默认进入 **总览**。顶栏控件不变并作用于所有 Tab：

- Agent 筛选（全部 / 单个）
- 周期：`month` | `quarter` | `year`

| Tab | 职责 |
|-----|------|
| 总览 | 本期费用 / Tokens / 调用；用量趋势；厂商费用 Top |
| 模型用量 | 厂商 / 模型 / Agent 明细；模型 Tokens 横条；精简次要 KPI |
| 工具技能 | 工具 / Skills / MCP / 定时（结构基本保持） |
| Agent 协作 | 编排列表与协作图（结构基本保持） |
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

## 模型用量 Tab 收敛

相对现状减少与总览的重复（以下为明确取舍，不再「或」）：

- **去掉**现有 6 枚 KPI 与整宽用量趋势（由总览承载）
- 顶部可选 **2 个次要 KPI**：模型数、活跃 Agent（可再更短，但不能回到 6 卡）
- **主内容：** 厂商 / 模型 / Agent 三列排行 + 模型 Tokens 横条
- **未计价提示只出现在总览**；模型 Tab 不再重复展示同一条

## 工具 / 协作 / Tracing

- 本期**不做**布局大改
- 仅接入新的五 Tab 导航与共享顶栏（Agent + 周期）
- 文案与空态保持现有语义

## 数据与 API

- **复用**现有 `get_usage_insights`（及协作 / Trace 既有命令）
- KPI / series / rankings 均从现有响应派生；厂商维度沿用当前前端聚合逻辑
- **不新增**后端字段或命令（本期）

## 前端落点

- `apps/desktop/src/components/InsightsPanel.tsx` — `ViewMode` 增加 `overview`（或等价默认值）；渲染总览区块；调整模型 Tab
- `apps/desktop/src/styles/insights.css` — 总览网格、KPI 三列、响应式；保持 overflow 防护
- `apps/desktop/src/i18n/messages.ts` — `insights.view.overview` 等中英键

## 验收

1. 进入洞察默认落在「总览」  
2. 有数据时，首屏约一屏内可见：3 KPI + 趋势 + 花钱 Top（宽屏）  
3. 切换周期 / Agent 后总览与其它 Tab 数据一致过滤  
4. 「更多」能切到模型用量 Tab  
5. 触控板横向滑动不再拖偏整页  
6. 工具 / 协作 / Tracing 功能回归不破  

## 非目标（本期不做）

- 预算告警、CSV 导出、自定义单价  
- 首屏塞协作摘要 / Tracing 摘要  
- 新查询 API 或预聚合表  
- 深链（滚动到指定排行行、URL 状态）  
- 用图表库替换纯 CSS/SVG 柱图  

## 测试要点

- 默认 `view === overview`（或产品等价默认）  
- 总览 KPI 仅三字段；空 series / 空 rankings 空态合理  
- 窄屏断点网格变为单列  
- i18n 中英键齐全  
