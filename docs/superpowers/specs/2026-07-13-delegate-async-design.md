# 异步委派 Delegate Async（Hermes 对齐）

**日期:** 2026-07-13  
**状态:** 已实现（MVP）  
**前置:** [delegate-sync](./2026-07-13-delegate-sync-design.md)

## 目标

父 Agent 调用 `delegate_async` 立刻拿到 `task_id`，后台跑现有 `delegate_exec`；用 `delegate_status` / `delegate_collect` / `delegate_cancel` 查询、等待或取消。

## 工具

| 工具 | 行为 |
|------|------|
| `delegate_async` | 参数同同步 `delegate`；返回 `{task_id,status:"running"}` |
| `delegate_status` | `{task_id}` → 状态 + 截断 summary/error |
| `delegate_collect` | 等到 done/failed/cancelled 或 timeout（默认 600s） |
| `delegate_cancel` | 标记取消；已完成则 no-op |

## 存储

进程内 `AsyncDelegateRegistry`（`OnceLock`）；可选旁路 JSON 于 `~/.astro/delegate_async/{id}.json` 便于调试。不上独立 SQLite（后置）。

## 执行

`set_delegate_async_spawner`：`tokio::spawn(run_delegate → registry.finish)`。

## 非目标

嵌套 orchestrator、子 HITL 上浮、进程重启续跑。
