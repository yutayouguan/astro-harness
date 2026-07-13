# multi_agent 工具瘦身

**日期:** 2026-07-13  
**状态:** 已实现  

## 目标

去掉 `multi_agent` 的 PLAN-ONLY JSON 占位；改为把 `goal` + `agents[]` 映射为 `orchestration_run` 等价步骤并真执行。

## 行为

- `agents[i]` → 编排一步：`role=agents[i]`，`prompt=As {role}, help achieve: {goal}`，临时角色（无 agent_id）
- 复用 `OrchestrationDb` + `request_orchestration_spawn`
- 返回与 `orchestration_run` 相同形态的 `{orchestration_id,status}`
- 描述引导：并行即时任务用 `delegate`；串行流水线用本工具或 `orchestration_run`

## 非目标

删除 toolset / 前端开关；改 `agent::Orchestrator` API（已接 delegate_exec，保留）。
