# cron

> 定时任务的完整生命周期管理：定义、调度解析、JSON 持久化、到期扫描与执行记录。

## 核心职责

1. **任务定义与持久化** -- 在 `~/.astro/automation/cron/jobs.json` 以 JSON 格式管理定时任务列表，支持 CRUD 操作（add / update / remove / enable / disable / archive），原子写入（临时文件 + rename）防半写损坏。归档是软删除：只停止自动调度，定义与运行记录都保留，恢复时按当前时间重算 `next_run_at`。
2. **调度表达式解析** -- 支持三种调度语法：`every:Nunit`（间隔调度，可选 `;wd=` 工作日过滤）、`custom:` 日历重复（每小时 / 天 / 周 / 月 / 年及自适应字段）、五段 cron（分 时 日 月 周，支持 `*` / 数字 / 逗号列表 / 区间）。

新建或更新 `custom:` 任务时会自动持久化本地墙钟相位
`start=YYYY-MM-DDTHH:MM`，确保“每 N 天/周/月/年”从任务配置时刻稳定计数，
编辑和重启后不会重新按 Unix epoch 对齐。
3. **到期扫描与触发** -- `tick()` / `claim_due()` 扫描已到期任务并推进 `next_run_at`。
4. **执行记录持久化** -- `CronRunDb`（`~/.astro/automation/cron/cron_v1.db`，WAL 模式）记录每次触发的运行状态（running / success / failure），支持按 job / agent / 日期过滤查询，`summary` 截断 2KB、`output` 截断 512KB。
5. **工具分发入口** -- `dispatch_cron_tool` 将 Agent 工具调用按 `action` 分支路由到 CronStore 的对应操作。

## 模块结构

| 文件 | 职责 |
|---|---|
| `src/lib.rs` | crate 入口；re-export `jobs::*` 与 `run_db` 公共类型 |
| `src/run_db.rs` | `CronRunDb` -- SQLite 执行记录层；`insert_running` / `finish_success` / `finish_failure` / `get` / `delete` / `delete_for_job` / `distinct_job_ids` / `has_running_for_job` / `list_running` / `list_filtered` |
| `src/jobs/mod.rs` | jobs 子模块入口；re-export 并包含模块级集成测试 |
| `src/jobs/model.rs` | 数据模型 -- `CronJob`（持久化定义）、`NewCronJob`（创建输入）、`CronJobExtract`（自然语言抽取目标）、`normalize_cron_extract`（校验与规范化）、`normalize_cron_agent_id`（legacy `"default"` 映射） |
| `src/jobs/store.rs` | `CronStore` -- 文件级持久化；`open` / `add` / `add_job` / `update_job` / `remove` / `set_enabled` / `set_archived` / `list` / `list_all` / `claim_due` / `tick` / `touch_last_run`；内含 `JobsFile` 结构和心跳文件写入 |
| `src/jobs/schedule.rs` | `compute_next_run` -- 调度表达式解析引擎；`every:` 间隔、`custom:` 日历重复、五段 cron（`CronField` 枚举 + 分钟级扫描） |
| `src/jobs/dispatch.rs` | `dispatch_cron_tool` -- Agent 工具入口；按 `action` 参数分发 add / list / remove / enable / disable（`list` 会把归档任务标为 `archived`） |
| `src/jobs/tick.rs` | `tick_default` -- 对默认 cron 目录执行一次 tick 的便捷入口 |

## 核心类型与 API

### 结构体

- `CronJob` -- 持久化任务定义（id / schedule / task / title / agent_id / provider_id / model / enabled / created_at / last_run_at / next_run_at / show_in_chat / archived_at）
- `NewCronJob` -- 创建或更新任务的输入
- `CronJobExtract` -- 自然语言抽取的结构化目标（实现 `JsonSchema`）
- `CronStore` -- 文件级持久化存储；管理 `jobs.json` 与 `output/` 目录
- `CronRunDb` -- SQLite 执行记录访问层
- `CronRunRow` -- 单次执行的完整记录行（id / job_id / title / agent_id / schedule / task / fired_at / finished_at / status / summary / output / error / session_id / trigger）
- `NewCronRun` -- 插入运行中记录的输入
- `CronRunFilters` -- 列表查询过滤条件（job_id / agent_id / date_from / date_to / limit）

### 关键函数

- `compute_next_run(schedule, after)` -- 解析调度表达式并计算下次运行时间
- `tick_default()` -- 对默认 cron 目录执行一次到期扫描
- `dispatch_cron_tool(args)` -- Agent 工具分发入口
- `normalize_cron_extract(draft)` -- 校验并规范化抽取结果
- `normalize_cron_agent_id(raw)` -- 规范化 agent_id（空 / `"default"` 映射为 `DEFAULT_AGENT_ID`）
- `cron_extract_preamble()` -- Extractor 前置约束文本
- `cron_dir()` -- 默认 cron 根目录路径
- `cron_db_path(cron_root)` -- 数据库文件路径

### 不变量

- 任务 id 为 UUID，持久化前校验 schedule 可解析
- `summary` 最长 2KB、`output` 最长 512KB（UTF-8 安全截断）

## 与其他 crate 的关系

| crate | 关系 |
|---|---|
| `home` (`agent-home`) | 路径约定（`default_memory_dir`、`ensure_default_workspace_dirs`、`DEFAULT_AGENT_ID`、`normalize_agent_id`） |
| `types` (`agent-types`) | SQLite 辅助（`open_wal`、`SqliteStore` trait、`truncate_utf8`） |
| `agent` (`agent-core`) | `exec::cron` 模块调用 `tick_default` 获取到期任务，然后通过 `run_background_multi_turn` 适配到统一 `SessionTask` 事件循环 |
| `server` (`agent-server`) | Cron ticker 在 side thread 以 `current_thread` runtime 运行，每 30s 调用 `tick_default`；gRPC 层提供任务管理 API |
| `usage` (`agent-usage`) | 执行时写入 `kind=cron` 用量事件 |

## 测试运行命令

```bash
# 全部测试（单元 + 集成）
cargo test -p cron

# 单个集成测试文件
cargo test -p cron --test cron_run_db_test

# 单个测试函数
cargo test -p cron every_schedule_and_tick
```
