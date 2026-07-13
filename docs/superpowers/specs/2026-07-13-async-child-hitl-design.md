# 异步委派子 HITL 上浮

**日期:** 2026-07-13  
**状态:** 已实现  
**前置:** [child-hitl-surface](./2026-07-13-child-hitl-surface-design.md)、[delegate-async](./2026-07-13-delegate-async-design.md)

## 目标

`delegate_async` 后台子 Agent 遇 `astro_hitl` 时，若父会话仍有**进行中的聊天流**，则挂到父 `HitlGate` 并推送 A2UI / `hitl_waiting`；否则仍 cancelled。

## 做法

1. `streaming`：聊天开始时 `register_live_parent_hitl(session_id, ParentHitlCtx)`，结束时 unregister  
2. `try_park_parent_hitl`：先 task_local（同步 delegate），再按 `parent_session_id` 查 live 表  
3. `delegate_exec`：把 `parent_session_id` 传入 park 路径  
4. 记录可选：`AsyncDelegateRegistry` 在 waiting 时标 `WaitingHitl`（便于 status）

## 非目标

父聊天已 Done 后的侧信道 HITL；编排步 HITL 上浮。
