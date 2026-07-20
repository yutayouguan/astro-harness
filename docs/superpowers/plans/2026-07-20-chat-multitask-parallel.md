# 聊天模式调度 — Step 2：MultiTask 并行 session

> 设计见 [`../specs/2026-07-20-chat-mode-scheduling-design.md`](../specs/2026-07-20-chat-mode-scheduling-design.md)

## 目标

MultiTask 下用户每发一句 → 立即用**新 `session_id`** 开一路 `start_chat`，多路并行；主时间线展示各 task 气泡 + Task 面板。

## 约束

- 同 `session_id` 不可并发（backend `register_pause` 重入会 cancel）
- 不走 follow-up 队列
- MVP 不强制 worktree

## 任务

- [x] 更新设计文档验收
- [x] `ParallelChatTask` 类型
- [x] MultiTask 发送路径：新 session + 独立 listen + `start_chat`
- [x] 多 listener Map，不拆主会话 unlisten
- [x] Task 面板 UI + i18n
- [x] 并发上限 5
- [x] 新会话清空并行任务

## 验收

见设计文档 §6b。
