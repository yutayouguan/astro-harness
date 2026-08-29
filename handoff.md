# Astro 代码缺陷交接

> 规则：在 Agent/Harness 对齐过程中发现代码缺陷时，先记录证据、影响、复现和建议，不直接修改实现。
>
> 更新时间：2026-08-29

## 状态约定

- `Open`：证据充分，尚未修复。
- `NeedsValidation`：存在风险信号，需要补充复现或测试。
- `FixedPendingVerification`：已有修复，尚未完成回归。
- `Closed`：修复和回归均完成。

## H-001 数据库迁移目标与默认打开路径不一致

| 字段 | 内容 |
| --- | --- |
| 状态 | `Closed` |
| 严重度 | High |
| 发现日期 | 2026-08-29 |
| 发现阶段 | Agent Harness 文档与源码对齐 |
| 是否已修复 | 是；已完成实现、回归与文档收敛 |

### 问题描述（修复前）

`agent-home::ensure_workspace_dirs()` 把多个旧数据库迁移到 `{base}/data/`，但当前部分默认 helper 和生产调用仍从旧路径打开数据库。迁移一旦发生，后续旧路径调用可能创建新的空数据库；与此同时，部分新调用已经使用 `{base}/data/`，从而形成同一进程或不同功能读取不同数据库的 split-brain。

### 源码证据（修复前）

迁移目标：

- `crates/agent-home/src/workspace/paths.rs:295-317`
  - `{base}/usage.db` → `{base}/data/usage.db`
  - `{base}/subagents-v2.db` → `{base}/data/subagents-v2.db`
  - `{base}/sessions/state.db` → `{base}/data/state.db`
  - artifacts、knowledge、cron 数据库也采用相同迁移方向。

仍使用旧路径的入口：

- `crates/agent-usage/src/db.rs:158-160`：`usage_db_path()` 返回 `{base}/usage.db`。
- `crates/agent-subagents/src/store.rs:36-38`：`v2_default_db_path()` 返回 `{base}/subagents-v2.db`。
- `crates/agent-server/src/grpc/astro_service.rs:39-44`：SessionStore 从 `{base}/sessions/state.db` 打开。
- `crates/agent-memory/src/agent/workspace/lifecycle.rs:20-28`：workspace 初始化仍创建 `{base}/sessions/state.db`。

已经使用新路径的入口：

- `crates/agent-tools/src/engine/context.rs`、`dispatch.rs` 以及 shell/media/HITL 工具中的多个调用从 `{base}/data/state.db` 打开 SessionStore。

迁移触发入口：

- `crates/agent-usage/src/stats.rs`
- `crates/agent-home/src/config/tools_enabled.rs`
- `crates/agent-cron/src/jobs/store.rs`
- `crates/agent-mcp/src/config.rs`

这些模块会调用 `ensure_default_workspace_dirs()`，因此问题不局限于显式数据迁移命令。

### 可能影响

1. 迁移后会话历史在桌面端或 Server 中看似消失，因为旧路径被重新创建为空 `state.db`。
2. 工具执行与主对话可能分别读写 `data/state.db` 和 `sessions/state.db`。
3. usage 和 Agent Graph 可能在新旧位置产生两份数据库，造成统计、mailbox 或恢复状态不一致。
4. 如果数据库正处于 WAL 模式，直接移动主文件而未协调连接与 sidecar 文件还需单独评估。

### 建议复现测试

只在临时 `ASTRO_MEMORY_DIR` 中执行：

1. 在旧路径创建带唯一记录的 `usage.db`、`subagents-v2.db` 和 `sessions/state.db`。
2. 调用 `ensure_workspace_dirs()`。
3. 分别调用 Usage、AgentGraph 和 Session 的默认生产入口。
4. 断言打开路径均为迁移后的同一 canonical 位置。
5. 断言唯一记录仍可见，且旧路径未生成第二份空库。
6. 增加 WAL/SHM 存在时的迁移测试，并验证进程重启后的恢复行为。

### 已采用的修复方向

- `agent-home` 集中定义 Session、usage、Agent Graph、artifacts、knowledge 和 Cron 运行库 resolver，canonical 目录统一为 `{base}/data/`。
- Usage、AgentGraph、Artifact、Knowledge 和 Cron 的默认入口先执行 workspace 迁移，Server、Desktop、Agent runtime 和辅助任务调用不再打开旧 Session 路径。
- SQLite 迁移将主库、`-wal`、`-shm` 作为文件族处理；单个文件族中途失败时回滚已移动成员。
- 旧库和 canonical 主库同时存在时直接报错，不覆盖任何一份数据；旧 `cron/cron.db` 迁移时同时重命名为 `data/cron_v1.db`。

### 验证结果

- `cargo check -p agent -p server -p subagents -p astro-agent`：通过。
- `cargo test -p home`：通过，包含 canonical resolver、SQLite 文件族迁移、split-brain 拒绝和旧 Cron 文件名迁移。
- `cargo test -p usage -p artifacts -p cron -p memory --lib`：通过。
- `cargo test -p usage --test usage_db_test migrated_legacy_usage_database_remains_visible -- --exact`：通过，确认迁移后 usage 记录可见且旧库不会重建。
- `cargo test -p agent --test cron_exec_test`：通过。
- `cargo test -p session --lib`：通过；`dispatch_session_tool_test` 的 2 个用例单独执行通过。
- `memory::ensure_workspace_migrates_legacy_session_without_creating_a_second_database` 验证迁移后原 Session 数据可见，且不重建旧库。

### 验收标准

- 同一职责数据库只有一个 canonical 路径。
- 所有默认 helper、Server、Desktop、Cron、工具和测试使用同一个 resolver。
- 旧路径迁移幂等，已有目标文件时有明确冲突策略。
- WAL/SHM、失败回滚和跨版本恢复均有覆盖。
- 迁移前后的会话、usage、Agent Graph 数据可见性一致。

## H-002 Agent Core test-support 未跟随异步存储 API

| 字段 | 内容 |
| --- | --- |
| 状态 | `Open` |
| 严重度 | Medium |
| 发现日期 | 2026-08-29 |
| 发现阶段 | H-001 相关 crate 回归测试 |
| 是否已修复 | 否；与本次数据库路径修复分离 |

### 问题描述

`crates/agent-core/src/exec/dispatch/test_support.rs` 的多个同步辅助方法仍直接对已经返回 `Future` 的 `AgentGraphStore`、`AgentControl` 和 `SessionStore` API 使用 `?`。当其他 crate 的测试通过 dev-dependency 启用 Agent test-support 时，`agent` 无法编译。

### 验证证据

执行：

```bash
CARGO_TARGET_DIR=/tmp/astro-harness-doc-check \
  cargo test -p home -p usage -p artifacts -p cron -p memory -p subagents
```

编译器在 `dispatch/test_support.rs` 的 `LifecycleTestApp::new`、`restart_with`、`dispatch_at`、`status`、`pending_mailbox`、`session_contents` 和 `session_ids` 等位置报告 `E0277`，提示应对返回的 Future 使用 `.await`，但所在方法当前不是 async。

### 建议处理

单独设计 test-support 的异步边界：将辅助 API 整体异步化，或通过明确的测试 runtime 适配；不要在零散调用点使用阻塞嵌套 runtime。修复后恢复 `subagents` 及依赖 `agent/test-support` 的测试覆盖。

## 后续缺陷记录模板

```markdown
## H-NNN 标题

- 状态 / 严重度 / 发现日期 / 是否已修复
- 问题描述
- 源码证据与调用链
- 用户影响和数据风险
- 最小复现
- 建议修复方向
- 验收标准
```
