# 嵌套委派 / 编排深度限制

**日期:** 2026-07-13  
**状态:** 已实现  
**前置:** [delegate-sync](./2026-07-13-delegate-sync-design.md)、[delegate-async](./2026-07-13-delegate-async-design.md)

## 目标

限制 `delegate` / `delegate_async` / `orchestration_run` 的嵌套深度，避免无限 fan-out。

## 语义

| 字段 | 含义 |
|------|------|
| `caller_depth` | 发起方当前深度（顶层聊天 = 0） |
| `max_spawn_depth` | 允许发起嵌套的最大 `caller_depth`（默认 **1**） |

- 顶层（0）可派生子（子运行在 depth=1）
- 当 `caller_depth >= max_spawn_depth` 时工具直接报错
- 子 Agent 运行时：`depth >= max` → strip `delegate*` / `orchestration_*`（叶子）；`depth < max` → 保留以便再嵌套

## 实现

1. `memory::spawn_depth`：`task_local` 上下文 + `can_spawn_nested()`
2. `DelegateRunRequest` / `OrchestrationSpawnRequest` 带 `caller_depth`、`max_spawn_depth`
3. `delegate_exec` / `orchestration`：scope depth + 深度感知 strip

## 非目标

改编排串并行；持久化 depth 到 DB；可配置 UI（常量默认即可）。
