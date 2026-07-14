# 可观测性对齐设计（S1：turn_id + 日志运维面）

**日期:** 2026-07-14  
**状态:** 已批准 / S1 已实现  
**范围:** `turn_id` 贯通、日志分层（agent / errors）、按 session/turn 过滤的最小运维 UI  
**外部参考（仅设计借鉴，不引入其品牌命名）：** 参考 Agent 运行时的「会话轨迹 + 文件日志 + insights」模式（非 OpenTelemetry 全家桶）

## 命名约束

- **代码、模块、类型、文件、用户可见文案、路径中不得出现 `hermes` / `Hermes` 字样。**
- 推荐命名：`turn_id`、`query_agent_logs`、`AgentLogQuery`、`agent.log` / `errors.log`、设置里「日志 / 诊断」。
- 本文档仅在「外部参考」处提及上游风格；正文用「参考架构」表述。

## 背景与现状

Astro 已具备参考架构同级的 **Agent 运行时** 可观测骨架：

| 能力 | 现状 |
|------|------|
| 会话轨迹 | `SessionStore`（`state.db`）为真相源；流式结束后写入富消息 |
| 用量 / Trace 聚合 | `UsageDb` + `trace_insights`（**session = 一条 Trace**）+ Insights 面板 |
| 生命周期钩子 | `hooks` 三套总线已火到 LLM / tool / session |
| 运行时事件到 UI | `streaming` → gRPC → Tauri `chat-stream-*`（`EventBus` 未接入生产，**保持不动**） |
| 文件日志 | `memory::init_logging(component)` → `~/.astro/logs/{component}.log` 按日滚动 |

缺口（相对运维痛点）主要是：

1. **回合级 ID 缺失**：`session_id` 已有，但同一会话多轮无法在日志/用量行上稳定关联  
2. **日志未分层**：桌面端多为单一 `astro-agent` / `backend` 文件，无 `errors` 专用面  
3. **无应用内按 session/turn 过滤日志**：排查依赖裸读文件或 `RUST_LOG`

## 目标

首期竖切（**S1**）同时服务「两者都要、先一小刀」：

1. **内核契约**：用户回合生成 `turn_id`，贯通 streaming emit、UsageDb、关键 `tracing` 字段  
2. **日志分层**：`agent.log`（INFO+）与 `errors.log`（WARNING+）  
3. **最小运维面**：偏好设置内「日志 / 诊断」分区，可按 `session_id` / `turn_id` 过滤最近日志  

成功标准：

- 发起一轮聊天后，日志行与当次 Usage 事件均带相同 `session_id` + `turn_id`  
- 在设置里粘贴当前 `session_id`（或 `turn_id`）能刷出该回合相关行  
- 不引入 OTEL SDK、Prometheus、旁路 log SQLite、不复活 `EventBus`

## 明确不做（S1）

- Insights 按 turn 下钻 UI（属 **S2**）  
- OpenTelemetry / Langfuse 内建导出（可选 **S2+** 经 Hooks 旁路）  
- `/health` HTTP、Prometheus 指标  
- RL / ShareGPT trajectory 导出  
- 将 JSONL 或 `tool-calls.jsonl` 提升为轨迹真相源  
- SessionStore compaction / `end_session` 语义补齐（另有 session 路线图）  
- 改写前端 localStorage 镜像策略（仅要求新字段不依赖它）

## 决策摘要

| 项 | 选择 |
|----|------|
| 实现路线 | **结构化 tracing 字段 + 扫日志文件尾部过滤**（非旁路 log 库） |
| Trace 粒度 | **session = Trace**（保持现状）；**turn = 回合 span 主键**（新增） |
| 日志文件 | `~/.astro/logs/agent.log` + `errors.log`；按日滚动保持 |
| Usage 存 `turn_id` | **正式列**（迁移加列，禁止因加列摧毁用量历史） |
| 运维 UI | `PreferencesPanel`（偏好设置）内「日志 / 诊断」分区 |
| Stream 事件 | **可选**附带 `turn_id`（前端 S1 可不展示） |

---

## 架构

```text
                     turn_id (每轮生成)
                            │
        ┌───────────────────┼───────────────────┐
        ▼                   ▼                   ▼
  AgentLoop / streaming   UsageDb 行         tracing fields
  MultiTurnStreamItem     turn_id 列         session_id + turn_id
        │                                        │
        ▼                                        ▼
  Tauri chat-stream                    ~/.astro/logs/
                                       ├── agent.log   (INFO+)
                                       └── errors.log  (WARN+)
                                                │
                                                ▼
                                   query_agent_logs (Tauri)
                                                │
                                                ▼
                                   Preferences「日志 / 诊断」
```

| 组件 | 职责 | 建议路径 |
|------|------|----------|
| `turn_id` 生成与持有 | 回合开始创建；线程/异步任务内传递 | `agent`：`loop_.rs` / `streaming.rs` |
| Usage 双写 | LLM/工具用量写入 `turn_id` | `agent/src/usage_record.rs` + `memory/src/usage/db.rs` |
| 日志初始化 | 双 sink（agent + errors）、字段友好格式 | `memory/src/infra/logging.rs` |
| 日志查询 | 读滚动文件尾部、过滤、截断 | `memory` 新模块如 `infra/log_query.rs` + Tauri command |
| 运维 UI | 过滤表单 + 只读行列表 | `PreferencesPanel` 或抽出 `DiagnosticsPanel` 嵌入偏好 |

---

## 契约：`session_id` 与 `turn_id`

### 定义

| 字段 | 含义 | 生成时机 | 生命周期 |
|------|------|----------|----------|
| `session_id` | 一次对话会话（已有） | 建会话 / 确保会话 | 跨多轮，直到 session 结束 |
| `turn_id` | **一轮用户输入 → 本轮 Done/Error** | `run_turn` / streaming 入口 | 仅当前回合；下轮新建 |

格式：短 UUID（无花括号、小写 hex，建议 8～32 字符，实现可选 `uuid::Uuid::new_v4()` 去掉 `-` 取前 16 亦可）。稳定性优先于可读性。

### 贯通要求（S1 必须）

1. **streaming / AgentLoop**  
   - 回合入口生成 `turn_id`  
   - 本回合内所有 `tracing::info!/warn!/error!` 在关键路径带 `session_id`、`turn_id`（优先 `span` 或显式字段，避免只拼进字符串）  
   - 工具执行、fallback、用量记录路径可读到同一 `turn_id`

2. **UsageDb**  
   - `usage_events` 增加可空列 `turn_id TEXT`  
   - `NewUsageEvent` / insert / `trace_insights` 读路径透传（S1 Insights UI 可不展示列，但 API 保留）  
   - `USAGE_SCHEMA_VERSION` 递增；**迁移必须 `ALTER TABLE … ADD COLUMN`（或等价声明式加列）**；禁止「版本不匹配即 DROP 毁库」用于本次升级

3. **Stream → 前端（可选）**  
   - `MultiTurnStreamItem` / proto / `ChatStreamEvent` 可带 `turn_id`  
   - S1 UI **不要求**展示；聊天区「复制诊断信息」可以后用

4. **Hooks（建议带、不强制改契约）**  
   - 若 hook payload 已有 `session_id`，同轮尽量附 `turn_id`，便于外挂观测；不阻塞 S1 验收

### 与现有 Trace 模型的关系

- `trace_insights`：**继续 session = Trace**  
- `turn_id`：用量行与日志的 **回合关联键**；S2 再据此做 Insights 下钻，S1 不改 Trace 列表产品语义

---

## 日志分层

### 路径与内容

| 文件 | 级别 | 内容 |
|------|------|------|
| `~/.astro/logs/agent.log` | INFO 及以上（受 `RUST_LOG` / 默认 filter 约束） | Agent 活动：回合开始、API、工具、会话生命周期 |
| `~/.astro/logs/errors.log` | WARNING 及以上 | agent 通道的错误镜像（过滤子集） |

说明：

- 桌面入口今日 `init_logging("astro-agent")`、backend `init_logging("backend")`：**S1 归一为 agent 语义文件名** `agent.log`（可保留 component 标签字段；避免再散落多套命名）  
- 若未来独立 gateway 进程，再增 `gateway.log`（**S1 不做**）  
- 继续按日滚动（`tracing_appender::rolling::daily`）；查询需覆盖当日及必要的滚动后缀策略（至少读「当前活动文件」；可选同前缀最近一份）

### 行格式约定

文件 layer：`with_ansi(false)`，带 `target`；**结构化字段**输出 `session_id`、`turn_id`（及既有 `component` 等），便于子串过滤：

```text
… session_id=20260714_… turn_id=a1b2c3d4e5f67890 …
```

不要求 JSON Lines；子串匹配即可（对齐「`logs --session`」体验）。

### 初始化 API 演进

`init_logging` 演进为双 writer（或两个 layer）：

- 共享 `EnvFilter`  
- layer A → `agent.log`  
- layer B → `errors.log`，另加 `LevelFilter::WARN`（或等价）

`WorkerGuard`：可保留单个 non-blocking 汇聚，或双 guard；须保证进程存活期间不 drop。

可安全重复 `try_init`（现状行为保留）。

---

## 日志查询 API

### 内存层

```rust
// 示意，非最终签名
pub struct AgentLogQuery {
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub min_level: Option<String>, // DEBUG|INFO|WARNING|ERROR（可选）
    pub lines: usize,              // 默认 50，上限例如 500
    pub source: LogSource,         // Agent | Errors | Both（默认 Both 或 Agent）
}

pub struct AgentLogLine {
    pub raw: String,
    pub source: String, // "agent" | "errors"
}

pub fn query_agent_logs(q: AgentLogQuery) -> anyhow::Result<Vec<AgentLogLine>>;
```

行为：

1. 打开 `logs_dir()` 下对应文件，自文件尾向前取足够原始行（实现可用环形缓冲 / 逆序读）  
2. 过滤：若设 `session_id` / `turn_id`，行必须包含对应子串  
3. `min_level`：尽力解析 tracing 行内级别；无法解析的行在「有 level 过滤」时仍可保留（避免拆行丢失）  
4. 返回时间正序或「最新在上」—— **UI 默认最新在上**，API 注明顺序  

性能：仅扫尾部，不做全库索引；大文件以 `lines` 与预读上限封顶。

### Tauri

- Command 名：`query_agent_logs`  
- 注册于现有 `config_commands` 或新建 `diagnostics_commands`  
- 输入/输出 JSON 与上表一致  

---

## 最小运维 UI

**入口：** 偏好设置（`PreferencesPanel`）新增分区：**日志 / 诊断**。

控件：

- `session_id` 文本框（可预填当前聊天 session，若 App 能传入）  
- `turn_id` 文本框（可空）  
- 来源：`agent` / `errors` / `全部`  
- 行数（默认 50）  
- 「刷新」按钮  
- 只读列表（等宽 / 可换行）；「复制全部」  

交互约束：

- 不做 Insights 跳转、不做 span 树  
- 无实时 tail（S1）；需要则用户再点刷新  
- 文案走 i18n（`MessageKey`）

---

## 分期

### S1（本期，本 spec）

- `turn_id` 生成 + agent 主路径贯通  
- UsageDb `turn_id` 列 + 安全迁移  
- 日志 `agent` / `errors` 双文件  
- `query_agent_logs` + 偏好设置诊断分区  

### S2（后续，另开 spec）

- Insights：选中 session 后按 `turn_id` 分组下钻 LLM→工具→费用  
- Stream/UI 展示 `turn_id`、一键「复制诊断上下文」  
- Hooks 旁路 Langfuse / OTEL exporter（可选插件）  
- `gateway.log`（若独立网关进程落地）  
- `/health`（若常驻后端进程需要）  

### S3+（明确更后）

- Prometheus / 完整 OTEL 三支柱内建  
- 旁路 SQLite log index  

---

## 测试计划

| 层 | 用例 |
|----|------|
| unit | `query_agent_logs`：合成日志行，按 session/turn/level 过滤正确；空文件；超大截断 |
| unit | UsageDb 迁移：旧库加列后历史行仍在，`turn_id` 为 NULL；新插入带值 |
| integration | 一轮 chat（或 agent 测试 harness）：usage 行与日志同时含相同 `turn_id` |
| UI 手测 | 偏好设置过滤当前 session；errors 来源仅见 WARN+ |

---

## 风险与缓解

| 风险 | 缓解 |
|------|------|
| Usage「毁库升级」丢历史 | 本迭代改迁移为 ADD COLUMN；测试覆盖 |
| 多进程双 `init_logging` | 保持 `try_init`；文件名统一后注意 desktop/backend 是否同写一文件——桌面主路径以 Tauri 进程为准 |
| 日志非 JSON、过滤漏报 | 强制关键路径结构化字段；文档说明「子串匹配」 |
| turn_id 未传到 tool 路径 | S1 验收清单点名 tool + llm 两条写入 |

---

## 与记忆对齐工作的关系

记忆系统对齐（`2026-07-14-memory-hermes-alignment-design.md`）与本 S1 **正交**：可并行，无共享 schema 冲突。实现排期上若冲突，优先不阻塞记忆 P1；本 S1 改动面集中在 `logging` / `usage` / `streaming` / Preferences。
