# Chat Right Panel Transparent Backdrop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 移除右侧栏打开时的视觉遮罩，同时保留点击侧栏外部关闭。

**Architecture:** 保留现有 `.chat-right-backdrop` 按钮作为全区域透明点击层，不修改 React 结构或关闭回调。仅移除其背景和淡入动画，并清理不再使用的关键帧。

**Tech Stack:** React 18、CSS、Vite

## Global Constraints

- 点击侧栏外部仍关闭侧栏。
- 关闭按钮与 `Esc` 行为保持不变。
- 对话区域不再出现变暗遮罩。
- 不修改 `ChatRightPanel.tsx`。

---

### Task 1: 将遮罩改为透明点击层

**Files:**
- Modify: `frontend/src/styles/features/chat/right-panel.css:22-33,612-626`

**Interfaces:**
- Consumes: `ChatRightPanel.tsx` 中 `.chat-right-backdrop` 的 `onClick={onClose}`。
- Produces: 无视觉背景、仍可点击关闭的透明覆盖层。

- [ ] **Step 1: 移除视觉遮罩**

将 `.chat-right-backdrop` 的视觉声明改为：

```css
.chat-right-backdrop {
  background: transparent;
}
```

删除该规则中的 `animation: chat-right-fade-in 0.18s ease;`，保留定位、层级、光标和圆角声明。

- [ ] **Step 2: 清理不再使用的动画**

删除：

```css
@keyframes chat-right-fade-in {
  from {
    opacity: 0;
  }
  to {
    opacity: 1;
  }
}
```

将 reduced-motion 规则收窄为：

```css
@media (prefers-reduced-motion: reduce) {
  .chat-right-panel {
    animation: none;
  }
}
```

- [ ] **Step 3: 运行前端构建**

Run: `cd frontend && npm run build`

Expected: TypeScript 与 Vite 构建成功，退出码为 0。

- [ ] **Step 4: 检查差异**

Run: `git diff --check && git diff -- frontend/src/styles/features/chat/right-panel.css`

Expected: 仅包含透明背景、遮罩动画移除和 reduced-motion 清理。

- [ ] **Step 5: 提交改动**

```bash
git add frontend/src/styles/features/chat/right-panel.css
git commit -m "style(chat): remove right panel backdrop"
```
