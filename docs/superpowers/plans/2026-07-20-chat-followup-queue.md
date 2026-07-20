# 聊天模式调度 — Step 1：Follow-up 队列

> 设计见 [`../specs/2026-07-20-chat-mode-scheduling-design.md`](../specs/2026-07-20-chat-mode-scheduling-design.md)

## 目标

Agent / Plan / Ask 在流式进行中可继续输入；发送后进入 Queued，当前 turn 结束后自动出队发送。

## 任务

- [x] 设计文档
- [x] `QueuedFollowUp` 类型 + 会话内队列状态
- [x] `send` 包装：streaming 且非 multitask → 入队并清空 composer
- [x] streaming 结束后 drain 队首
- [x] ChatView：Queued 列表（展开、编辑、删除）
- [x] i18n / CSS
- [x] MultiTask 本步不入队

## 验收

见设计文档 §6。
