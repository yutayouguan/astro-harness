# 异步委派跨重启持久化

**日期:** 2026-07-13  
**状态:** 已实现  
**前置:** [delegate-async](./2026-07-13-delegate-async-design.md)、[orchestration-resume](./2026-07-13-orchestration-resume-design.md)

## 目标

进程重启后，对仍为 `running` 的 async 委派重新 spawn（需落盘完整 `DelegateRunRequest`）。

## 做法

1. 启动时旁路写入 `{id}.json`（状态）+ `{id}.request.json`（凭据与 tasks）
2. Agent 注册 async spawner 后：扫描 `running` 旁路文件 → `start`/`spawn` 续跑（取消标志则标 cancelled）
3. 完成/失败时更新状态文件

## 非目标

独立 SQLite；跨机器迁移。
