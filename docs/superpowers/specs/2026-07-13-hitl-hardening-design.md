# HITL 加固（阻塞闸门后）

**日期:** 2026-07-13  
**状态:** 实现中  
**前置:** [hitl-blocking-multitool](./2026-07-13-hitl-blocking-multitool-design.md)

## 范围

1. Resume payload：轻量 JSON Schema 校验（`type` / `required` / `properties` 基础子集）
2. 同批多 HITL：并存多个 oneshot，resume 可一次解析多个 id
3. 编排子步遇 HITL：不 park；tool result 记为 cancelled，步骤可继续
4. 危险命令三级：`deny` / `ask` / `auto`（白名单自动批，无辅模型）
5. 清理废弃 InterruptStore 活表路径；补测试

## 非目标

辅模型 smart、子 Agent HITL 上浮父卡片、派生协作重写
