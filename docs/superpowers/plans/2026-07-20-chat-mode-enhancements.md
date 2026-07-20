# 聊天模式增强：拒绝续跑 / checkpoint / Agent worktree

> 设计见 [`../specs/2026-07-20-chat-mode-scheduling-design.md`](../specs/2026-07-20-chat-mode-scheduling-design.md) §6e

## 已做

1. 取消 `request_mode_switch` → 注入拒绝说明并续跑
2. 长任务空闲 60s + 有排队 → 暂停回合并出队
3. Agent 模式按 session 复用 git worktree（`project_root`）
