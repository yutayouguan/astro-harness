# Chat Right Panel Edge Alignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让右侧对话侧栏的顶部、右侧和底部与对话容器完全齐平。

**Architecture:** 保持现有绝对定位浮层结构，移除右侧栏顶部、右侧和底部方向的内缩。宽度、遮罩、动画和滚动逻辑不变。

**Tech Stack:** CSS、Vite

## Global Constraints

- 仅修改 `apps/desktop/src/styles/features/chat/right-panel.css`。
- `.chat-right-panel` 使用 `top: 0`、`right: 0` 与 `bottom: 0`。
- 保持宽度、圆角、阴影、遮罩和滑入动画不变。

---

### Task 1: 对齐右侧栏上下边缘

**Files:**
- Modify: `apps/desktop/src/styles/features/chat/right-panel.css:40-60`

**Interfaces:**
- Consumes: `.chat-layout-with-right` 的相对定位边界。
- Produces: 与对话容器等高的 `.chat-right-panel` 浮层。

- [x] **Step 1: 修改上下偏移**

将定位声明改为：

```css
.chat-right-panel {
  position: absolute;
  top: 0;
  right: 0;
  bottom: 0;
}
```

保留该规则中的其余声明。

- [x] **Step 2: 运行前端构建**

Run: `cd frontend && npm run build`

Expected: TypeScript 与 Vite 构建成功，退出码为 0。

- [x] **Step 3: 检查样式差异**

Run: `git diff --check && git diff -- apps/desktop/src/styles/features/chat/right-panel.css`

Expected: `top`、`right` 与 `bottom` 均为 `0`，无空白错误。

- [x] **Step 4: 提交改动**

```bash
git add apps/desktop/src/styles/features/chat/right-panel.css
git commit -m "fix(chat): align right panel edges"
```
