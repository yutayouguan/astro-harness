# Chat Right Panel Edge Alignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让右侧对话侧栏的顶部和底部与左侧对话容器完全齐平。

**Architecture:** 保持现有绝对定位浮层结构，只移除右侧栏上下方向的 10px 内缩。右侧间距、宽度、遮罩、动画和滚动逻辑不变。

**Tech Stack:** CSS、Vite

## Global Constraints

- 仅修改 `frontend/src/styles/features/chat/right-panel.css`。
- `.chat-right-panel` 使用 `top: 0` 与 `bottom: 0`。
- 保持 `right: 10px`、宽度、圆角、阴影、遮罩和滑入动画不变。

---

### Task 1: 对齐右侧栏上下边缘

**Files:**
- Modify: `frontend/src/styles/features/chat/right-panel.css:40-60`

**Interfaces:**
- Consumes: `.chat-layout-with-right` 的相对定位边界。
- Produces: 与对话容器等高的 `.chat-right-panel` 浮层。

- [ ] **Step 1: 修改上下偏移**

将定位声明改为：

```css
.chat-right-panel {
  position: absolute;
  top: 0;
  right: 10px;
  bottom: 0;
}
```

保留该规则中的其余声明。

- [ ] **Step 2: 运行前端构建**

Run: `cd frontend && npm run build`

Expected: TypeScript 与 Vite 构建成功，退出码为 0。

- [ ] **Step 3: 检查样式差异**

Run: `git diff --check && git diff -- frontend/src/styles/features/chat/right-panel.css`

Expected: 仅 `top` 与 `bottom` 从 `10px` 改为 `0`，无空白错误。

- [ ] **Step 4: 提交改动**

```bash
git add frontend/src/styles/features/chat/right-panel.css
git commit -m "fix(chat): align right panel edges"
```
