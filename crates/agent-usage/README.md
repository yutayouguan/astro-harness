# usage

> 用量事件持久化、per-agent 统计、路由感知成本估算与调用链 Tracing 的一站式用量库。

## 核心职责

1. **事件持久化** -- 在 `~/.astro/usage/usage.db`（WAL 模式，schema v4）记录 tool / skill / mcp / cron / llm 五类用量事件，支持按月/季/年时间窗的 KPI 聚合、时间序列与多维排行查询。
2. **工具集与技能计数** -- 以 `~/.astro/usage/agents/{id}/stats.json` 维护 per-agent 的工具集调用次数和技能调用次数，通过互斥锁保证进程内并发安全。
3. **路由感知费用估算** -- 结合官方文档定价快照（OpenAI / Anthropic / Google 等）与 OpenRouter `/models` API 缓存（24h TTL），为每次 LLM 调用输出 USD 估价或 `included` / `unknown` 状态。
4. **调用链 Tracing** -- 按 `session_id` 聚合用量事件并尝试从 `state.db` 会话消息重建 LangSmith 风格 span 链（含 input/output、耗时、parent 关系），为前端可观测性面板提供数据。
5. **Eval 导出** -- 将单会话 Trace 导出为 JSONL 格式（一行一条记录），便于离线评测与 DSPy 集成。

## 模块结构

| 文件 | 职责 |
|---|---|
| `src/lib.rs` | crate 入口；re-export 全部公共类型与函数 |
| `src/db.rs` | `UsageDb` -- SQLite 事件写入（`insert` / `try_record`）、schema 迁移（v3->v4 ADD COLUMN）、按 period/agent 的洞察聚合（KPI + series + rankings）、Trace 会话列表与事件时间线 |
| `src/stats.rs` | `AgentUsageStats` / `AgentUsageSummary` -- JSON 文件级工具集与技能调用计数；`record_tool_call` 串行写盘并双写 `usage.db` |
| `src/pricing.rs` | 路由感知费用估算 -- `resolve_billing_route` 判定计费路由（OfficialSnapshot / ProviderModelsApi / Included / Unknown），`estimate_usage_cost` 计算 USD 金额；含 OpenRouter 缓存读写 |
| `src/trace_insights.rs` | `query_trace_insights` -- 按 session 聚合调用链；优先从 `SessionStore` 的 chat history 构建 span（含 I/O 与耗时），再合并 `usage.db` 的 token/费用；`propagate_turn_ids` 回填 turn 标识 |
| `src/eval_export.rs` | `export_session_eval_jsonl` / `write_eval_record_jsonl` -- session trace 到 JSONL 的导出，可注入 `UsageDb` 实例便于测试 |

## 核心类型与 API

### 结构体

- `UsageDb` -- SQLite 访问层；`new(path)` / `open_default()` / `insert(NewUsageEvent)` / `try_record(NewUsageEvent)` / `query_insights(UsageInsightsQuery)`
- `NewUsageEvent` -- 插入事件的输入（ts / kind / name / agent_id / token 四桶 / cost_usd / billing 字段等）
- `UsageInsights` -- 聚合查询结果（`UsageKpis` + `Vec<UsageSeriesPoint>` + `UsageRankings` + `unpriced_llm_events`）；KPI 同时包含 LLM 调用数以及输入、输出、缓存、推理 Token 构成，供 Desktop 展示费用覆盖率与用量结构。
- `UsageKpis` -- 汇总指标：calls / tokens / cost_usd / active_agents / llm_calls / input_tokens / output_tokens / cache_tokens / reasoning_tokens
- `AgentUsageStats` -- JSON 级工具集与技能计数快照
- `AgentUsageSummary` -- 面向 API 的用量摘要（含 tool_total / skill_total）
- `UsageTokens` -- 四桶 token 用量（input / output / cache_read / cache_write / request_count）
- `CostResult` -- 费用估算结果（amount_usd / status / source / pricing_version / label）
- `TraceInsights` / `TraceSummary` / `TraceEvent` -- Tracing 洞察结构
- `EvalSessionRecord` / `EvalEvent` / `EvalMessagePreview` -- Eval 导出结构

### 枚举

- `UsagePeriod` -- 洞察时间粒度：Month / Quarter / Year
- `CostStatus` -- 费用状态：Estimated / Included / Unknown
- `BillingRoute` -- 计费路由：OfficialSnapshot / ProviderModelsApi / Included / Unknown

### 关键函数

- `record_tool_call(agent_id, tool_name, args, session_id, turn_id)` -- 记录工具调用并双写 JSON + usage.db
- `estimate_usage_cost(model, usage, provider, base_url, api_key)` -- 路由感知费用估算
- `resolve_billing_route(model, provider, base_url)` -- 判定计费路由
- `query_trace_insights(TraceInsightsQuery)` -- 查询调用链 Tracing 洞察
- `export_session_eval_jsonl(session_id, path)` -- 导出单会话 Eval JSONL
- `period_window(period, as_of)` -- 返回时间窗半开区间

## 与其他 crate 的关系

| crate | 关系 |
|---|---|
| `home` (`agent-home`) | 路径约定（`default_memory_dir`、`agent_config_dir`、`tool_name_to_toolset`） |
| `types` (`agent-types`) | SQLite 辅助（`open_wal`、`SqliteStore` trait、`truncate_chars`） |
| `session` (`agent-session`) | Tracing 与 Eval 导出时读取 `SessionStore` 的会话消息 |
| `agent` (`agent-core`) | Agent 运行时在每轮 LLM 完成后调用 `record_tool_call` / `try_record` 写入用量 |
| `server` (`agent-server`) | gRPC 层通过 `UsageDb` 提供用量查询 API |

## 测试运行命令

```bash
# 全部测试（单元 + 集成）
cargo test -p usage

# 单个集成测试文件
cargo test -p usage --test usage_db_test

# 单个测试函数
cargo test -p usage estimate_usage_cost_official_snapshot
cargo test -p usage skill_rows_do_not_inflate_calls -- --nocapture
```
