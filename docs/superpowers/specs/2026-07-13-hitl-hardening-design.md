# HITL 加固（阻塞闸门后）

**日期:** 2026-07-13  
**状态:** 已实现  
**前置:** [hitl-blocking-multitool](./2026-07-13-hitl-blocking-multitool-design.md)

## 范围

1. Resume payload：轻量 JSON Schema 校验（`type` / `required` / `properties` 基础子集）
2. 同批多 HITL：并存多个 oneshot，resume 可一次解析多个 id
3. 编排子步遇 HITL：不 park；tool result 记为 cancelled，步骤可继续
4. 危险命令三级：`deny` / `ask` / `auto`（白名单自动批，无辅模型）
5. 清理废弃 InterruptStore 活表路径；补测试

## 非目标

辅模型 smart、子 Agent HITL 上浮父卡片、派生协作重写  
（辅模型降级见另文 [smart-approval](./2026-07-13-smart-approval-design.md)，非本规格强制项。）

## 实现说明

| 项 | 落地 |
|----|------|
| Schema 校验 | [`crates/agent-core/src/schema_validate.rs`](../../../agent/src/schema_validate.rs)；`HitlGate::resolve` 对 `resolved` 强制校验 |
| 同批多 HITL | `HitlGate` 多 oneshot；`begin_wait` 不取消 sibling；`interrupt_resume` 可一次 resolve 多 id；旁路 `interrupt.json` 仅写剩余 pending |
| 编排 HITL | [`crates/agent-core/src/orchestration.rs`](../../../agent/src/orchestration.rs) 遇 `astro_hitl` 改写为 cancelled 文案，不 park |
| deny/ask/auto | [`crates/agent-tools/src/approval.rs`](../../../tools/src/approval.rs) + streaming terminal 路径 |
| 活路径 | 生产 resume → `HitlRegistry`；`interrupt.json` 仅为 UI/调试旁路，不再作假续跑真相源 |

## 验收

- [x] schema 非法 resume 被拒绝  
- [x] 一次 resume 可完成多个 pending；sibling 不被误清  
- [x] 编排路径 HITL 不阻塞子步  
- [x] `rm -rf node_modules` → Auto；`mkfs` → Deny；一般 `rm -rf` → Ask  
- [x] cancel / timeout 清理 waiting
