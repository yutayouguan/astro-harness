# 子 Agent HITL 上浮到父会话

**日期:** 2026-07-13  
**状态:** 已实现  
**前置:** [hitl-blocking](./2026-07-13-hitl-blocking-multitool-design.md)、[delegate-sync](./2026-07-13-delegate-sync-design.md)

## 目标

同步 `delegate` 子 Agent 遇 `confirm`/`clarify`（`astro_hitl`）时，挂到**父会话** `HitlGate`，父流发 A2UI + `hitl_waiting`；用户 resume 后子继续。

## 做法

1. `streaming` 串行工具执行时用 `task_local` 注入 `ParentHitlCtx { gate, tx, run_id }`。
2. `delegate` 标为 exclusive，保证走串行路径且带 gate。
3. `delegate_exec`：遇 `astro_hitl` 则 `park_astro_hitl`（reason/message 加 `[delegate]` 前缀）；无 ctx 则 cancelled。
4. 子 Agent **恢复** `clarify`/`confirm`（不再 strip）。

## 非目标

`delegate_async` / `orchestration_run` HITL 上浮；改 gRPC 协议。
