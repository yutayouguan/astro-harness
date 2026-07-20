# 聊天模式调度 — Step 4：软边界 / 汇总 / worktree

> 设计见 [`../specs/2026-07-20-chat-mode-scheduling-design.md`](../specs/2026-07-20-chat-mode-scheduling-design.md)

## 目标

1. HITL 软边界：停顿期间可入队；仅整轮 Done/Error 后自动出队（`turnInFlight`）。
2. MultiTask 本地汇总条 + 写入对话 + 清除已结束。
3. MultiTask 有 git 仓时自动 `create_task_worktree`，经 `project_root` 下传。

## 验收

见设计文档 §6d。
