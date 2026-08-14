# 真委派 Delegate（Hermes 对齐 · 同步 MVP）

**日期:** 2026-07-13  
**状态:** 已实现  
**范围:** 升级 `delegate` 为同步真 spawn 子 AgentLoop；可并行最多 3 个子任务；摘要回父  
**非目标:** 异步 task_id、嵌套 orchestrator、子 HITL 上浮、durable 队列

## 行为

- 工具 `delegate`：`goal`+`context` 或 `tasks[{goal,context?}]`；兼容旧 `task` 字段
- 每个子任务：新 session、父凭据、剔除 `delegate` / `orchestration_*` / `clarify` / `confirm` / `memory_*`
- `JoinSet` 并行，默认 `max_concurrent=3`；父工具调用阻塞至全部结束
- 返回合并摘要 JSON；失败项带 error，不拖垮整批（单项 failed）

## 模块

| 位置 | 职责 |
|------|------|
| `memory/delegate_spawn.rs` | sync runner（经 `ToolContext` 注入，非全局 OnceLock） |
| `agent/delegate_exec.rs` | 真执行 |
| `tools/builtin/delegate.rs` | 参数 + 调 runner |
| `agent/multi_agent.rs` | Orchestrator 接真执行器 |
