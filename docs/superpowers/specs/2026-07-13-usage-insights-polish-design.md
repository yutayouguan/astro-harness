# 洞察近期补强设计

**日期:** 2026-07-13  
**状态:** 已批准  
**范围:** Usage Insights MVP 三处缺口（错误路径 LLM 记账、Cron LLM 用量、趋势图指标切换）  
**前置:** `docs/superpowers/specs/2026-07-13-usage-insights-design.md`

## 目标

补齐洞察数据与展示的准确性：流式失败时不丢已累计 token/费用；定时任务执行模型时写入真实 LLM 用量；趋势图可切换 calls / tokens / cost。

## 背景

终审与代码核对结论：

- `finish_usage_and_done` 会写 `kind=llm`；`finish_error` 只发 Error+Done，已累计 `total_usage` 丢失。
- `cron_exec::run_provider_loop` 忽略 `chunk.usage`；成功仅写 `kind=cron` 且 tokens/cost=0。
- `InsightsPanel` 柱图仅用 `series.calls`，API 已返回 `tokens` / `cost_usd`。

## 变更 1：流错误路径记 LLM usage

**行为：** 凡调用 `finish_error` 且本轮/本流已累计非空 `Usage` 时，先按与 `finish_usage_and_done` 相同口径 `UsageDb::try_record`（`kind=llm`、model、tokens、`estimate_llm_cost`），再 emit Error + Done。

**实现要点：**

- 扩展 `finish_error` 签名（或抽 `record_llm_usage_if_any` 辅助函数），传入 `session`、`model`、`Option<Usage>`。
- 所有现有 `finish_error` 调用点传入当时可用的 `total_usage` / `saw_usage`（无则 `None`，行为与今相同）。
- 失败可忽略，不阻塞错误上报。

**非目标：** 不改变前端错误展示文案；不在无 usage 时写空事件。

## 变更 2：Cron 执行记真实 LLM 用量

**行为：**

1. `run_provider_loop` 在 chunk 循环中累加 `chunk.usage`（覆盖式或累加式，对齐 providers 流约定）。
2. 任务成功时：
   - 保留现有 `kind=cron` 事件（调用次数维度）。
   - 若累计 usage 非空，再写一条 `kind=llm`（`name`=模型 id，`cost_usd`=`estimate_llm_cost`，`agent_id`/`session_id` 与 cron 一致）。
3. 任务失败且已有部分 usage：同样尽力写 `kind=llm`（与变更 1 一致）；`kind=cron` 仍仅在 success 时写（保持现口径）。

**非目标：** 不把 cron 整段改为 `run_multi_turn_stream`；不回填历史 cron 记录。

## 变更 3：趋势图指标切换

**行为：** 洞察面板趋势区增加指标切换：`calls` | `tokens` | `cost`（默认 `calls`）。柱高按所选字段相对 `max` 缩放；标题与 `title` tooltip 显示对应数值；费用仍标注「估」。

**实现要点：**

- 本地 state，无需新 Tauri API。
- i18n 中英键（如 `insights.metric.calls` / `tokens` / `cost`）。
- 样式对齐现有 period tabs，避免新图表依赖。

## 验收

1. 流中途失败但已有 usage → `usage.db` 有对应 `llm` 行。
2. 到期/手动 cron 成功跑模型 → 有 `cron` +（有 usage 时）`llm` 行，费用非零（价目表命中时）。
3. 洞察页可切换三项指标，柱图高度随数据变化。
4. 现有 `usage_db_test` / 聊天正常结束路径回归通过。

## 非目标

- 历史回填、自定义单价、协作 2D/3D、CSV、预算告警（仍属二期+）。
