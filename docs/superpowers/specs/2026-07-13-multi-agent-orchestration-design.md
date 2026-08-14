# 多 Agent 编排竖切设计（Orchestration MVP）

**日期:** 2026-07-13  
**状态:** 已批准 / 已实现  
**范围:** A3 完整编排的最小竖切——父任务串行派生子 Agent、可查状态、handoff 遥测；不上并行 / 2D / 外部队列  
**关联:** `docs/superpowers/specs/2026-07-13-usage-insights-design.md`（协作可视化后置）

## 目标

让主 Agent 通过工具发起一次**异步串行编排**：立刻返回 `orchestration_id`，后台按步骤执行子 Agent（已有 Agent 或临时角色），状态持久化可查询，并为后续 Insights 协作图留下遥测边。

## 背景与约束

- 现有 `multi_agent` / `delegate` 主要写 workspace JSON（`planned` / `queued`），不真正调度。
- `agent::Orchestrator::dispatch_parallel` 为占位实现。
- `usage_events` 尚无 handoff 边；Insights 协作 2D 依赖本竖切的数据基础。
- 产品为单机 Tauri：编排用**进程内 tokio + SQLite**，不上 Redis 等外部队列。
- 长期入口含 Tauri API；**MVP 仅工具触发**。

## 方案选择

采用**独立 `orchestration.db` + 新工具 `orchestration_run` / `orchestration_status`**，与 `usage.db` 分离；步骤完成时尽力写 `usage_events` 遥测。不采用「仅扩展 JSON 文件」或「只用 usage_events 当状态机」。

## 数据模型

路径：`~/.astro/orchestration.db`（rusqlite，WAL；风格对齐 `usage.db` / `cron.db`）。

### 表 `orchestrations`

| 列 | 说明 |
|---|---|
| `id` TEXT PK | UUID |
| `parent_agent_id` TEXT NOT NULL | 发起方 |
| `session_id` TEXT | 可选父会话 |
| `goal` TEXT NOT NULL | 总目标 |
| `status` TEXT NOT NULL | `queued` → `running` → `done` \| `failed` \| `cancelled` |
| `created_at` / `updated_at` / `finished_at` TEXT | ISO UTC |
| `error` TEXT | 失败摘要 |
| `result_summary` TEXT | 完成后总述 |

### 表 `orchestration_steps`

| 列 | 说明 |
|---|---|
| `id` TEXT PK | UUID |
| `orchestration_id` TEXT NOT NULL | FK |
| `seq` INTEGER NOT NULL | 串行序号 `0..N-1` |
| `role` TEXT NOT NULL | 角色名（展示） |
| `agent_id` TEXT | 已有 Agent；空 = 临时角色 |
| `prompt` TEXT NOT NULL | 该步指令 |
| `status` TEXT NOT NULL | `pending` → `running` → `done` \| `failed` \| `skipped` |
| `output` TEXT | 截断结果（建议上限 64KB） |
| `error` TEXT | |
| `started_at` / `finished_at` TEXT | |

索引：`(orchestration_id, seq)`、`(status, updated_at)`（便于日后扫描）。

## 工具 API

### `orchestration_run`

- **参数：** `goal: string`，`steps: [{ role, prompt, agent_id? }]`（长度 1～8）
- **行为：** 插入 orchestration（`queued`）+ steps（`pending`）→ `tokio::spawn` 执行器 → **立即**返回 `{ orchestration_id, status: "queued" }`
- **归类：** toolset 建议 `multi_agent` 或新 `orchestration`（实现时二选一并在 enabled 表登记）

### `orchestration_status`

- **参数：** `orchestration_id: string`
- **返回：** orchestration 状态字段 + 各 step 的 `seq/role/agent_id/status` 与截断 `output`/`error`

旧工具 `multi_agent` / `delegate`：**MVP 并存、不破坏**；描述中引导使用新工具。后续可内部转调。

## 执行器

模块：`crates/agent-core/src/orchestration.rs`（执行）+ `crates/agent-memory/src/orchestration_db.rs`（持久化）。

1. 将 orchestration 标为 `running`
2. 按 `seq` 串行：
   - step → `running`
   - 解析目标并执行（见下）
   - 成功：写 `output` → `done`；失败：step `failed`，orchestration `failed`，**停止后续**
3. 全部成功：orchestration `done`，`result_summary` 由各步输出摘要拼接
4. 单步超时：默认 **120s**（常量）；超时视为 failed
5. 同进程内同一 `orchestration_id` 不重复 spawn（内存去重或 DB 条件更新）

### 子 Agent 解析

对齐 `cron_exec` 使用 `AgentLoop` + 父/目标 Agent 凭据：

| `agent_id` | 行为 |
|---|---|
| 非空 | 切到该 Agent workspace/记忆；短多轮执行 `prompt`（轮次上限如 3～5） |
| 空（临时角色） | **不** `create_agent`；临时 `session_id`；上下文注入 `role` + `goal` + 上一步截断 `output`；用**父 Agent** 凭据与工具 |

`cancelled` 状态预留；MVP 无 UI 取消。

## 遥测

每步开始/结束尽力写入 `usage_events`（失败可忽略）：

- 建议 `kind = "orchestration"`（若实现成本高，可暂用 `kind=tool` + `name=multi_agent`）
- `meta_json`：`orchestration_id`、`step_id`、`seq`、`from`（parent）、`to`（agent_id 或 `role:…`）、`phase`（`start`/`end`）

供后续 Insights 协作 2D 使用；本竖切不做图 UI。

## 模块落点

| 位置 | 职责 |
|---|---|
| `crates/agent-memory/src/orchestration_db.rs` | 建库、CRUD、状态迁移 |
| `crates/agent-core/src/orchestration.rs` | 串行执行器、临时角色会话 |
| `tools` builtins | 注册 `orchestration_run` / `orchestration_status` |
| `usage_db` / 写入钩子 | 编排遥测边 |
| Tauri | MVP 不强制；二期面板/API |

不强制改写现有 `Orchestrator::dispatch_parallel` 占位；新路径独立。

## 验收

1. `orchestration_run` 立即返回 id，不长时间阻塞父聊天。  
2. `orchestration_status` 可见 step 状态推进。  
3. 指定 `agent_id` 与临时角色两种 step 至少各跑通一步。  
4. 串行顺序正确；一步失败则后续不执行。  
5. DB 有完整记录；遥测尽力写入。  
6. 旧 `multi_agent` / `delegate` 仍可用。

## 非目标（后置）

- 并行扇出、依赖图调度  
- 进程重启续跑、外部消息队列  
- Tauri「协作任务」面板、Insights 2D/3D  
- 完整取消 UX、子 Agent 独立凭据配置 UI  
- 历史 JSON 编排回填  

## 测试要点

- `orchestration_db`：创建、状态迁移、按 seq 查询  
- 执行器：mock/假 Provider 下串行与失败中止  
- 工具：空 steps、超过 8 步校验  
- 临时角色不创建 `workspace-*` 目录  

## 后续子项目（原 A3 拆分）

1. ~~本竖切：模型 + 串行执行 + 工具~~（本文）  
2. 并行 / 依赖图调度  
3. 重启续跑 + 可选 tick  
4. Insights 协作 2D + Tauri API  
