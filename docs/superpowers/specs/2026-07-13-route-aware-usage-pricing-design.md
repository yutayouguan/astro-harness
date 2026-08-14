# 路由感知用量与费用估算

**日期:** 2026-07-13  
**状态:** 已实现  
**实现分支:** `feat/route-aware-usage-pricing`  
**取代:** [Session Store P1a billing](./2026-07-13-session-store-p1a-billing-design.md)（该文档标记为已取代；冲突处以本文为准）  
**关联:** [Usage Insights](./2026-07-13-usage-insights-design.md)、[Session Store](./2026-07-13-session-store-design.md)  
**外部参考（正文不再重复品牌名）:** NousResearch agent `usage_pricing` / CanonicalUsage 语义

## 命名约束

- 代码、模块、类型、文件、用户可见文案、路径中不得出现上游项目品牌字样。
- 推荐命名：`CanonicalUsage` / 扩展现有 `Usage`、`BillingRoute`、`PricingEntry`、`CostResult`、`estimate_usage_cost`、`normalize_usage`、`update_session_billing`。

## 决策摘要

| 项 | 选择 |
|----|------|
| 范围 | 完整对齐参考架构的用量桶 + 路由定价 + cost 状态（对话选项 A） |
| 价目 | 路由定价：官方快照 / 兼容端 `/models` API / 订阅 `included`（选项 1）；**费用路径不再读 LiteLLM** |
| 历史 | 选项 C：重建 `usage.db`、清零 sessions 账单列、清空 `usage-stats.json`；不回填 |
| 权威写入 | **双写**：`usage_events`（`kind=llm`）+ `sessions` billing 列 |
| Insights | 继续以 `usage_events` 聚合为主（含 llm）；sessions 供会话级展示与后续扩展 |
| 对账 | MVP 不做 `actual`；OpenRouter 类为 models API **估算** |

相对已取代的 P1a：恢复向 `usage.db` 写 `llm`；Insights 不改为「LLM 只读 sessions」；cache/reasoning 不再固定为 0；`cost_source` 不再为 `litellm_public`。

## 目标

1. Provider usage 归一为四桶（+ reasoning 展示），按路由查价写入估算费用与状态。  
2. 流式 / cron 结束时双写事件表与会话账单列。  
3. 洞察 KPI 正确处理 `unknown`（不把未计价当成 $0 免费）。  
4. 升级后观测数据从零开始。

## 非目标

- Generation / 发票级 `actual_cost` 对账  
- 聊天内 `/usage` 条、子 Agent 费用树 UI  
- Compaction / session split（P1b）  
- 删除 LiteLLM 模型列表缓存文件（仅与费用解耦）  
- 回填历史消息 usage  

## 方案选择

在「就地扩展 providers+memory」「新建独立 pricing crate」「Insights 改读 sessions 弱化事件表」中采用 **就地扩展**：`providers` 负责归一化，`memory` 负责定价与持久化，`agent` 负责写入挂钩。

---

## 1. 数据模型

### `providers::Usage`（CanonicalUsage 语义）

| 字段 | 说明 |
|------|------|
| `input_tokens` | 未缓存新输入 |
| `output_tokens` | 输出 |
| `cache_read_tokens` / `cache_write_tokens` | 缓存读写 |
| `reasoning_tokens` | 展示/落库；**不单独乘价** |
| `request_count` | 默认 1；累加时相加 |

兼容别名：`prompt_tokens` = `input + cache_read + cache_write`；`completion_tokens` = `output`；`total_tokens` = prompt + output。

`parse_openai_usage` / Anthropic 等路径：OpenAI/Codex 类需从含 cache 的 prompt/input 总额中减去 cache 得到 `input_tokens`（与参考实现一致）。

### `usage_events`（重建，不兼容旧行）

路径仍为 `~/.astro/usage.db`。新列：

- Token：`input_tokens`、`output_tokens`、`cache_read_tokens`、`cache_write_tokens`、`reasoning_tokens`、`total_tokens`（可冗余存储便于 SUM）
- 费用：`cost_usd`、`cost_status`（`estimated` \| `included` \| `unknown`）、`cost_source`、`pricing_version`
- 路由：`billing_provider`、`billing_base_url`（可选 `billing_mode`）
- 废弃旧专用列：`prompt_tokens` / `completion_tokens`（新 schema 不再使用）

`kind=tool|skill|mcp|cron`：token/billing 字段默认 0/空。  
`kind=llm`：填写完整账单字段。

### `sessions`（v11 列真正启用）

累加：`input/output/cache_*/reasoning`、`estimated_cost_usd`、`api_call_count`、`billing_*`、`cost_status`、`cost_source`、`pricing_version`；`actual_cost_usd` 恒空。  
会话行不替代 Insights 主聚合源。

---

## 2. 路由定价

模块：`crates/agent-memory/src/usage/pricing.rs`（可拆 `billing_route`）。

### 公式

```text
cost =
  input      × input_$/M  / 1e6
+ output     × output_$/M / 1e6
+ cache_read × read_$/M   / 1e6
+ cache_write× write_$/M  / 1e6
+ request_count × request_$   （若有）
```

某桶非零且缺对应单价 → 整次 `unknown`，`cost_usd = 0`。

### `resolve_billing_route(model, provider, base_url)`

| 条件 | 行为 |
|------|------|
| 订阅类路径（若产品具备） | `included`，金额 0 |
| OpenRouter / 匹配其 host | 拉 models API；缓存 `~/.astro/openrouter-model-pricing.json`（TTL 建议 24h） |
| Anthropic / OpenAI / Google / Bedrock / MiniMax 等直连 | 内置官方价快照（覆盖项目常用模型，可增量扩充） |
| 其它 `base_url` 且 `/models` 含 pricing | 同 models API |
| localhost / custom / 查无 | `unknown` |

### `CostResult`

`amount_usd`、`status`、`source`、`pricing_version`、`label`（`~$x.xx` / `included` / `n/a`）。  
MVP 不做 `actual`。OpenRouter 路径文档注明 models API 估算。

### LiteLLM

费用路径 **禁止** 再读 `litellm-model-meta.json`。模型列表若仍用该文件，与定价解耦。删除或改写 `estimate_llm_cost` → `estimate_usage_cost(...)`。

---

## 3. 写入路径

### 流式（`streaming.rs`）

1. chunk usage → normalize → `total_usage.add_assign`  
2. `finish_usage_and_done` / `finish_error`（有非空 usage）时：  
   - `estimate_usage_cost(model, usage, provider, base_url, api_key?)`  
   - `UsageDb::try_record(kind=llm, …)`  
   - `SessionStore::update_session_billing(session_id, BillingDelta { … })`  
3. 必须传入当前 chat 的 **provider + base_url**  
4. 仍 emit `FinalUsage`（四桶；旧前端可读 total）

粒度：每个 `run_multi_turn_stream` 结束一条事件 + 一次会话累加（与现 `record_llm_usage` 一致）。

### 会话 `cost_status` 合并

- 本 run `estimated`/`included`：累加金额（included 加 0）  
- 本 run `unknown`：金额不加；会话若曾全部 known，可升为 `unknown`（「最差」优先）

### Cron

一次 run：`kind=cron`（次数）+ `kind=llm`（token/cost）；定价同路由 API。

### 工具 / MCP / skill / orchestration 遥测

次数/边照旧；费用 0。失败只 `warn`，不阻断对话。

---

## 4. 清空与迁移（选项 C）

### `usage.db`

- 引入 schema version（`user_version` 或 meta 表）  
- 旧库无版本 / 版本落后：**删除文件或 DROP 后按新 DDL 重建**，不迁移行  
- 启动日志：`usage.db rebuilt, prior events discarded`

### Sessions 账单列

- 一次性：`UPDATE sessions` 将 token/cost/billing/api_call_count 清零或置 NULL（保留 messages）  
- session `SCHEMA_VERSION` bump + 迁移标记，避免每次启动重复清零

### 其它

- 删除或清空 `~/.astro/usage-stats.json`  
- 不删 `litellm-model-meta.json`  
- 不回填历史；测试用临时目录

---

## 5. Insights / UI

- `tokens`：llm 按四桶之和（或 `total_tokens`）SUM  
- `cost_usd`：仅累加 `cost_status IN ('estimated','included')`；**排除 `unknown`**  
- 「部分未计价」：存在 `kind=llm AND cost_status=unknown`  
- 费用文案继续标「估」；趋势 calls/tokens/cost 不变  
- DTO 形状尽量兼容；后端聚合逻辑按上调整

---

## 测试计划

- `normalize_usage`：OpenAI 含 cache 拆分、缺字段、累加  
- `estimate_usage_cost`：快照命中、included、缺价 → unknown、四桶公式  
- `UsageDb`：新 schema insert；insights 忽略 unknown 金额  
- 迁移：旧 usage.db 打开后重建为空  
- `update_session_billing`：多次 delta；unknown 不抬高金额  
- 流式/cron 挂钩：写入带 `cost_status`（单测或 mock）

## 验收标准

1. 新对话 llm 事件含四桶 + 路由定价元数据。  
2. 未知模型为未计价，不冒充免费。  
3. 升级后旧洞察数据消失，面板从空态开始。  
4. LiteLLM 不参与 `cost_usd`。  
5. 仓库实现命名无上游品牌字样。  
6. sessions billing 列随 run 正确累加。

## 实现顺序建议

1. 扩展 `Usage` + `normalize_usage` + 单测  
2. 重写 `usage/pricing.rs`（快照 + 路由 + 可选 models 缓存）  
3. 重建 `usage.db` schema + 迁移摧毁逻辑  
4. `update_session_billing` + sessions 账单清零迁移  
5. 替换流式/cron `record_llm_usage` 双写  
6. Insights 聚合与前端未计价提示  
7. 更新/作废 P1a 与父 session-store 交叉引用  

## 风险

- 长会话若将来改读 sessions 做时间桶，归因仍粗糙（本文 Insights 以事件 `ts` 为准，风险较低）  
- 清空不可恢复（默认执行；文档可提示备份）  
- OpenRouter 缓存失效或离线时落入 unknown  
- 与仓库其它 WIP 并行时建议独立 worktree  
