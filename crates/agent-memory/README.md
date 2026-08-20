# memory

Astro Agent 记忆子系统：管理精炼记忆（MEMORY.md）、用户档案（USER.md）、待审批记忆队列、dreaming 入梦管道、决策日志与权限审计。

## 核心职责

- 提供 `MemoryManager` 统一入口：绑定单 Agent 工作区，管理 MEMORY.md / USER.md 的读写与快照
- `MemoryStore` 实现 Markdown 格式记忆的解析、条目增删改、快照刷新
- 待审批记忆队列（`pending`）：异步写入记忆前先入队，需用户或 Agent 审批
- Dreaming 入梦管道：定时回顾历史会话，提炼新记忆写入 MEMORY.md
- 记忆回顾（`review`）：LLM 辅模型审查已有记忆，提出清理/合并/更新建议
- 决策日志（`decision_log`）：记录 Agent 每次重要决策（记忆写入、审批、学习等）
- 权限审计（`permission_audit`）：append-only 日志，记录权限变更与授权事件
- 配置加载：记忆、压缩、学习、进化、辅助任务、审批等多维度配置
- 工作区引导（`ensure_workspace`）：初始化 `~/.astro` 目录结构

## 模块结构

| 文件/目录 | 职责 |
|-----------|------|
| `agent/store.rs` | `MemoryStore` — Markdown 记忆存储：解析条目、增删改、live/snapshot 双视图 |
| `agent/workspace.rs` | `ensure_workspace` / `ensure_default_workspace` — 工作区目录引导与 Agent 空间初始化 |
| `session/manager.rs` | `MemoryManager` — 单 Agent 记忆聚合入口：MEMORY/USER 存储、配置加载、prompt 内容生成 |
| `config.rs` | 多维度配置加载：`MemoryConfig` / `CompressionConfig` / `LearningConfig` / `EvolutionConfig` / `AuxiliaryConfig` / `ApprovalsConfig` / `LoadedPermissionSettings` |
| `pending.rs` | 待审批记忆队列：`enqueue` / `approve` / `reject` / `list_pending` — JSON 文件持久化 |
| `dreaming/mod.rs` | Dreaming 入梦管道：`prepare_all_dream_jobs` / `DreamRunReport` / `DreamingState` |
| `review.rs` | 记忆回顾：`build_review_digest` / `parse_review_llm_output` / `apply_review_suggestions` |
| `decision_log.rs` | 决策日志：`append_decision` / `list_recent` — JSONL 格式 |
| `permission_audit.rs` | 权限审计：append-only JSONL 日志、8MB 轮转归档、分页查询 |
| `protocol.rs` | 记忆协议定义 |

## 核心类型与 API

- `MemoryManager` — 单 Agent 记忆聚合入口
  - `new(base_dir)` — 以活跃 Agent 构造
  - `for_agent(base_dir, agent_id)` — 为指定 Agent 构造
  - `refresh_memory_snapshot()` — 刷新 MEMORY/USER 快照
  - `prompt_content()` — 返回 `(project_memory, user_profile)` prompt 片段
  - `dispatch_memory_tool(target, action, args)` — 记忆工具调用分发
- `MemoryStore` — Markdown 记忆存储
  - `parse_memory_entries(content)` — 解析 Markdown 为结构化条目
  - `MemoryWriteResult` — 写入结果（添加/替换/删除）
- `MemoryTarget` — 记忆写入目标：`Memory`（MEMORY.md）/ `User`（USER.md）
- `PendingMemoryWrite` — 待审批记忆条目
  - `enqueue` / `approve` / `reject` / `list_pending`
- `DreamingState` — 入梦状态：上次运行时间、统计
  - `prepare_all_dream_jobs` — 准备所有 Agent 的入梦任务
  - `DreamRunReport` / `DreamMemoryUpdate` — 入梦运行报告
- `ReviewSuggestion` / `ReviewOutput` — 记忆回顾建议与输出
- `DecisionEntry` / `DecisionKind` — 决策日志条目与类别
- `PermissionAuditEvent` / `PermissionAuditKind` — 权限审计事件
- 配置类型：`MemoryConfig` / `CompressionConfig` / `LearningConfig` / `EvolutionConfig` / `AuxiliaryConfig`
- `ensure_workspace(base_dir)` — 工作区引导：创建目录结构、初始化会话库、播种 Skill

## Crate 关系

| 方向 | crate | 说明 |
|------|-------|------|
| 依赖 | `types` | 共享类型与权限配置 |
| 依赖 | `home` | 路径约定（agent_workspace_dir / daily_memory_path） |
| 依赖 | `session` | 会话库初始化（ensure_workspace 调用） |
| 依赖 | `skills` | Skill 播种（ensure_workspace 调用） |
| 被依赖 | `agent`（agent-core） | 运行时通过 MemoryManager 管理记忆与配置 |
| 被依赖 | `tools`（agent-tools） | 记忆工具调用 dispatch_memory_tool |

## 配置体系

`config.rs` 从 `{base_dir}/config.yaml` 加载多维度运行时配置：

| 配置类型 | 说明 |
|----------|------|
| `MemoryConfig` | 记忆行为：自动刷新、nudge 阈值、每日记忆开关 |
| `CompressionConfig` | 上下文压缩策略：protect_last_n、protect_first_messages、mid_run 摘要 |
| `LearningConfig` | 学习循环：nudge 开关、自动 skill 建议 |
| `EvolutionConfig` | 自进化参数：评判、信号分析、DSPy 集成、搜索 |
| `AuxiliaryConfig` | 辅助任务路由：Dreaming/Compaction/SmartApproval/TitleGen 各自目标 |
| `ApprovalsConfig` | 审批策略：auto/manual/smart 模式、write_approval 开关 |
| `LoadedPermissionSettings` | 权限配置：预设 profile、沙箱模式、文件系统/网络策略 |

## 关键不变量

1. **双视图一致性**：`MemoryStore` 维护 live 与 snapshot 两份视图；live 反映磁盘当前状态，snapshot 为 prompt 注入时的冻结版
2. **审批前不落盘**：新记忆写入先进 `pending` 队列，审批通过后才写入 MEMORY.md
3. **权限审计不可变**：append-only JSONL，只追加不修改，8MB 自动轮转（最多 3 个归档文件）
4. **工作区幂等**：`ensure_workspace` 可重复调用，已存在的目录/文件不会被覆盖
5. **Agent 隔离**：每个 Agent 有独立的 workspace_dir，记忆互不干扰
6. **决策日志追踪**：每次记忆写入、审批、拒绝操作均记录 `DecisionEntry`，支持事后审计

## 测试

```bash
cargo test -p memory
```
