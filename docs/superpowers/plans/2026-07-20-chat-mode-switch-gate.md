# Plan：聊天模式 Step 3 — Plan gate + 模式切换授权

> 日期：2026-07-20  
> 规格：[`../specs/2026-07-20-chat-mode-scheduling-design.md`](../specs/2026-07-20-chat-mode-scheduling-design.md)

## 目标

1. 将 `interaction_mode` 从前端下传到 `AgentLoop`；
2. Plan / Ask：schema 过滤 + `file_ops` 写操作硬拦；
3. `request_mode_switch` 工具 + 流结束后授权条（倒计时 / 立即 / 取消）。

## 验收

- Plan 下模型看不到 `terminal` / `code_exec` / `delegate`；调用 `file_ops(write)` 返回 blocked；
- Agent↔Plan 可请求切换；流结束后出现授权条；倒计时结束或点「立即」切换；取消留在原模式；
- Plan→Agent 批准后注入 `summary` 再继续；
- 同轮只弹一次。
