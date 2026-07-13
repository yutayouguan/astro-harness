# Session Store P1a — 会话 LLM 计费（Billing）

**日期:** 2026-07-13  
**状态:** 已批准（对话确认）  
**前置:** [session-store P0](./2026-07-13-session-store-design.md)（schema v11 列已齐）  
**后续:** P1b Compaction / session split（另开 spec，本轮不做）  
**范围:** 将会话级 LLM token / 估算费用落到 `sessions` billing 列；Insights 的 LLM 视图改读 sessions 聚合；停止向 `usage.db` 写 `kind=llm`；清空历史 `usage_events`

## 命名约束

- 代码、模块、类型、文件、用户可见文案中不得出现 `hermes` / `Hermes`。
- 使用既有命名：`SessionStore`、`update_session_billing`、`BillingDelta`、`estimate_llm_cost`。

## 决策摘要

| 项 | 选择 |
|----|------|
| P1 拆分 | Billing（本 spec）先于 Compaction（P1b） |
| 权威源 | LLM 用量与费用以 **sessions** 为准 |
| 历史 | **清空** `usage_events` 全表；不回填进 sessions |
| `usage.db` 职责 | 继续写 tool / skill / mcp / cron；**不再**写 `llm` |
| 费用 | 仅 LiteLLM 公开价 **估算**；`actual_cost_usd` 恒空；`cost_status=estimated` |
| 实现路径 | **A**：会话累计计数器（无第三套计费明细表） |
| Insights | 保持 `UsageInsights` DTO；按 kind **分流合并**读源 |

## 目标

1. 每轮（run）LLM 结束后，把本 run 的 token delta 与估算费用累加到当前 `session_id` 的 `sessions` 行。  
2. Insights 面板中费用 / tokens / 模型排行反映 **P1a 之后** 的 sessions 数据；工具类排行仍来自 `usage.db`。  
3. 不引入新的计费事件表；不实现对账 / 发票 / Compaction。

## 非目标

- Compaction / split / `parent_session_id` 谱系逻辑（P1b）  
- `actual_cost_usd` 实算与对账流水  
- `llm_usage_events` 或任何第三套计费 schema  
- Gateway（P2）  
- 回填历史 `usage.db` → sessions  
- 扩展 `Usage` 的 cache / reasoning 细分（本期 delta 中对应列为 0）  
- 改写 HITL / delegate 等无关 WIP

## Schema（沿用 v11，不升版本）

已存在列（只写、不改结构）：

- Token：`input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens`, `reasoning_tokens`, `api_call_count`  
- Billing：`billing_provider`, `billing_base_url`, `billing_mode`, `estimated_cost_usd`, `actual_cost_usd`, `cost_status`, `cost_source`, `pricing_version`  
- 可选同步：`model`

## 写入时序与 API

### 触发点

与今日流式收尾一致：`finish_usage_and_done` / `finish_error` 在拿到本 run 累计 `Usage` 后调用一次落账（整 run 合计，非每半轮）。

### `record_llm_usage` → `apply_session_llm_usage`

1. 若 `prompt_tokens` / `completion_tokens` / `total_tokens` 全 0 → return  
2. `cost = estimate_llm_cost(model, prompt, completion)`（复用现价目缓存）  
3. `SessionStore::update_session_billing(session_id, BillingDelta { … })`  
4. **不再**调用 `UsageDb::try_record(kind=llm)`  
5. 失败只记日志，不阻断 Done（保持「尽力写入」）

### `BillingDelta`

| 字段 | 来源 |
|------|------|
| `input_tokens` / `output_tokens` | `Usage.prompt_tokens` / `completion_tokens` |
| `cache_read_tokens` / `cache_write_tokens` / `reasoning_tokens` | 本期固定 `0` |
| `estimated_cost_usd` | LiteLLM 估算 |
| `api_call_count` | `+1` |
| `billing_provider` / `billing_base_url` / `billing_mode` | 当前 Agent chat 配置写入/覆盖 |
| `cost_status` | `"estimated"` |
| `cost_source` | `"litellm_public"` |
| `pricing_version` | 价目缓存版本（有则写，无则 `NULL`） |
| `model` | 可选更新 `sessions.model` |

### SQL 语义

```text
UPDATE sessions SET
  input_tokens       = COALESCE(input_tokens, 0)  + ?delta_in,
  output_tokens      = COALESCE(output_tokens, 0) + ?delta_out,
  cache_read_tokens  = COALESCE(cache_read_tokens, 0) + 0,
  cache_write_tokens = COALESCE(cache_write_tokens, 0) + 0,
  reasoning_tokens   = COALESCE(reasoning_tokens, 0) + 0,
  estimated_cost_usd = COALESCE(estimated_cost_usd, 0) + ?delta_cost,
  api_call_count     = COALESCE(api_call_count, 0) + 1,
  billing_provider / billing_base_url / billing_mode = 本次元数据,
  cost_status = 'estimated',
  cost_source = 'litellm_public',
  pricing_version = ?version,
  model = COALESCE(?, model)   -- 按实现选择覆盖策略
  -- actual_cost_usd 不动
WHERE id = ?session_id
```

取消/错误路径：若已有部分 usage，仍按现逻辑落一次。

## Insights 读路径

### 门面

Tauri `get_usage_insights` 仍返回既有 `UsageInsights`（`kpis` / `series` / `rankings`）。前端字段形状尽量不变。

### 分流合并（`memory` 内门面，例如扩展 `query_usage_insights`）

| 指标 | 来源 |
|------|------|
| `rankings.by_model` | **仅 sessions**：按 `model` 聚合 `api_call_count`、`input_tokens+output_tokens`、`estimated_cost_usd`；时间窗 `started_at ∈ [start,end)`（epoch 与 period 边界对齐） |
| `rankings.by_kind` | **usage.db**，排除 `llm` |
| `rankings.by_agent` | **usage.db** 非 llm 为主；sessions 无可靠 `agent_id` 时不把 LLM 硬塞进 by_agent |
| `kpis.cost_usd` / `kpis.tokens` | sessions 窗口 `SUM(estimated_cost_usd)` + `SUM(input_tokens+output_tokens)`，加上 usage 非 llm 的 tokens/cost |
| `kpis.calls` | usage 非 llm calls（tool/mcp/cron）+ sessions `SUM(api_call_count)` |
| `kpis.active_agents` | usage 非 llm 的 distinct `agent_id` |
| `series` | 按 bucket **合并**：usage 非 llm 序列 + sessions 按 **`started_at` 落入 bucket** 贡献该会话累计 tokens / cost / `api_call_count` |

### Agent 过滤

带 `agent_id` 时：usage 非 llm 过滤照旧；sessions LLM 部分 **本期不计入**（避免错算），直至 sessions 有可对齐的 agent 元数据。

### 已知限制

Approach A 仅会话级累计、无逐轮流水 → 跨多日长会话的 LLM 费用整笔落在 `started_at` 所在时间桶。短聊可接受；P1a 不加明细表。

## 历史数据清空

1. P1a 首次启用时一次性执行：`DELETE FROM usage_events`（整表，含旧 llm / tool / skill 等）。  
2. 在 usage 库 meta（或等价键值）写入 `p1a_usage_events_cleared=1`，保证幂等。  
3. **不**迁移旧事件到 sessions；不清 `sessions` 消息正文；billing 列本就多为空则无需额外清。  
4. Insights 自清空后重新累计；实现前可在文档注明用户可自行备份 `~/.astro/usage.db`（默认仍执行清空）。

## 与 Usage Insights / 父 Session Store 的关系

- 父 spec 要求：不允许再发明第三套计费 schema → 本设计遵守。  
- Insights 继续双库读取，但 **LLM 权威在 sessions**；`usage.db` 降为非 LLM 事件库。  
- 旧 `kind=llm` 行因整表清空而不再存在；代码路径亦停止写入。

## 与 P1b（Compaction）边界

- 本 spec 不实现 split / lineage。  
- Billing 始终累加在**当前** `session_id`。  
- 将来 split 后，父子会话各自累计；是否拷贝父费用到子会话由 P1b 决定（默认不自动拷贝）。

## 测试计划

- `update_session_billing`：多次 delta 累加正确；`actual_cost_usd` 仍为 null；元数据字段写入  
- Agent 流式结束：sessions 列有值；`usage_events` 无新 `kind=llm`  
- 清空：首次删除全部事件；再次打开幂等  
- Insights 合并：纯 LLM / 纯工具 / 混合；`by_model` 来自 sessions  
- 带 `agent_id` 过滤不 panic、不把 LLM 重复计入错误维度  

## 验收标准

1. 新对话至少一轮 LLM 后，对应 session 的 token / `estimated_cost_usd` / `api_call_count` 正确累加。  
2. `usage.db` 不再新增 `kind=llm`；上线后历史 `usage_events` 已被清空。  
3. Insights：费用与模型排行反映新 sessions 数据；工具类图表仍可用。  
4. 仓库新增命名无 `hermes` 字样。  

## 实现顺序建议

1. `SessionStore::update_session_billing` + 单测  
2. Agent `apply_session_llm_usage` 替换 `record_llm_usage`  
3. usage 库一次性清空 + meta 门控  
4. Insights 查询门面分流合并 + 单测  
5. 前端文案（可选）：空态「自会话计费起统计」  
6. 更新父 session-store spec 状态备注：P1a 已开独立 spec / 实现中  

## 风险

- 长会话时间序列归因粗糙（见上）  
- 清空 `usage_events` 不可恢复（默认执行；文档提示备份）  
- 实现时 `main` 可能有无关 HITL WIP → 使用独立 worktree  
