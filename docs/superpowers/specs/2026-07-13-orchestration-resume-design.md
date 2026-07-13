# 编排重启续跑

**日期:** 2026-07-13  
**状态:** 已实现  

## 目标

进程重启后，对 `queued` / 崩溃留下的 `running` 编排重新 spawn，并从**未完成步骤**继续。

## 做法

1. `orchestrations` 表增加 `provider/model/api_key/base_url`（创建时写入）
2. `reclaim_stale_running`：把卡在 `running` 的步骤改回 `pending`
3. `run_orchestration`：跳过已 `done`/`skipped` 步骤；`try_claim` 失败则尝试 reclaim
4. 后端启动：`list_incomplete` → `request_orchestration_spawn`（无凭据则跳过）

## 非目标

加密 api_key；UI 手动「继续」按钮。
