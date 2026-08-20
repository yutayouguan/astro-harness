# Header and Session Toolbar Visual Unification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 统一聊天页顶部控件高度，并让会话侧栏工具栏采用一致的玻璃拟态样式。

**Architecture:** 保持 React 组件和交互逻辑不变，只在现有功能级 CSS 中增加局部尺寸约束和视觉覆盖。顶部栏继续使用现有 `--header-chip-*` 变量；会话工具栏沿用 Agent 选择器已建立的渐变、边缘高光、阴影和模糊语言。

**Tech Stack:** React 18、TypeScript、CSS、Vite

## Global Constraints

- 顶部状态、模型选择器和操作按钮组统一为 40px 外部高度。
- 会话侧栏三项继续保持 36px 高度。
- 搜索、新建会话和 Agent 选择器统一使用玻璃拟态；“新建会话”保留蓝色文字与图标。
- 不修改组件结构、状态逻辑或交互行为。

---

### Task 1: 统一顶部栏控件高度

**Files:**
- Modify: `apps/desktop/src/styles/features/shell/header.css:14-26,86-109,791-804`

**Interfaces:**
- Consumes: `App.tsx` 中现有 `.status-chip`、`.model-picker-trigger`、`.chat-header-tools` 结构。
- Produces: 三个顶部控件均为 40px 外部高度，不改变内部图标热区。

- [x] **Step 1: 记录当前高度差异**

在聊天页检查三个控件的 computed height。预期修改前 `.status-chip`、`.model-picker-trigger` 与 `.chat-header-tools` 至少有一项高度不是 40px。

- [x] **Step 2: 添加统一尺寸约束**

在 `header.css` 中为三个外层加入以下约束，并保持其余现有声明：

```css
.chat-header-tools {
  box-sizing: border-box;
  height: 40px;
}

.model-picker-trigger {
  box-sizing: border-box;
  height: 40px;
}

.status-chip {
  box-sizing: border-box;
  height: 40px;
}
```

- [x] **Step 3: 运行前端构建**

Run: `cd frontend && npm run build`

Expected: TypeScript 与 Vite 构建完成，退出码为 0。

- [x] **Step 4: 检查顶部视觉**

在浅色和深色主题下打开聊天页，确认三个控件 computed height 均为 `40px`，垂直中心线一致，模型图标和三个操作按钮未被裁切。

- [x] **Step 5: 提交顶部高度改动**

```bash
git add apps/desktop/src/styles/features/shell/header.css
git commit -m "fix(chat): align header control heights"
```

### Task 2: 统一会话工具栏玻璃样式

**Files:**
- Modify: `apps/desktop/src/styles/features/chat/right-panel.css:170-273`

**Interfaces:**
- Consumes: `.chat-session-toolbar` 内现有 `.expandable-search-btn`、`.expandable-search-field`、`.chat-session-new` 和 `.agent-picker-chip`。
- Produces: 三项共享相同的 36px 高度、12px 圆角、玻璃渐变、边缘高光、阴影和模糊效果。

- [x] **Step 1: 建立工具栏局部玻璃变量**

向 `.chat-session-toolbar` 添加：

```css
.chat-session-toolbar {
  --session-toolbar-glass-fill:
    linear-gradient(155deg, rgba(255, 255, 255, 0.42) 0%, rgba(255, 255, 255, 0.14) 100%),
    color-mix(in srgb, var(--tone-soft, transparent) 45%, rgba(255, 255, 255, 0.18));
  --session-toolbar-glass-fill-hover:
    linear-gradient(155deg, rgba(255, 255, 255, 0.52) 0%, rgba(255, 255, 255, 0.2) 100%),
    color-mix(in srgb, var(--tone-soft, transparent) 58%, rgba(255, 255, 255, 0.22));
  --session-toolbar-glass-edge:
    color-mix(in srgb, var(--tone, #64748b) 16%, rgba(255, 255, 255, 0.5));
  --session-toolbar-glass-shadow:
    0 6px 16px color-mix(in srgb, var(--tone, transparent) 8%, rgba(15, 23, 42, 0.05)),
    0 1px 3px rgba(15, 23, 42, 0.03),
    inset 0 1px 0 rgba(255, 255, 255, 0.55),
    inset 0 0 0 0.5px rgba(255, 255, 255, 0.28);
}
```

- [x] **Step 2: 应用一致的默认样式**

让搜索控件、新建会话按钮和 Agent 芯片共享：

```css
.chat-session-toolbar .expandable-search-btn,
.chat-session-toolbar .expandable-search-field,
.chat-session-toolbar .chat-session-new,
.chat-session-toolbar .agent-picker-chip {
  box-sizing: border-box;
  height: 36px;
  border: 1px solid var(--session-toolbar-glass-edge);
  border-radius: 12px;
  background: var(--session-toolbar-glass-fill);
  box-shadow: var(--session-toolbar-glass-shadow);
  backdrop-filter: blur(16px) saturate(1.2);
  -webkit-backdrop-filter: blur(16px) saturate(1.2);
}
```

删除 `.chat-session-new` 中冲突的边框和背景声明，保留 `color: var(--tone-blue, #2563eb)`。

- [x] **Step 3: 统一 hover 反馈**

为搜索按钮、新建会话按钮和 Agent 芯片使用同一 hover 背景及边框，并保留 Agent 选择器打开态：

```css
.chat-session-toolbar .expandable-search-btn:hover,
.chat-session-toolbar .chat-session-new:hover,
.chat-session-toolbar .agent-picker-chip:hover:not(:disabled),
.chat-session-toolbar .agent-picker-chip.is-open {
  background: var(--session-toolbar-glass-fill-hover);
  border-color:
    color-mix(in srgb, var(--tone, #64748b) 32%, rgba(255, 255, 255, 0.75));
}
```

- [x] **Step 4: 运行前端构建**

Run: `cd frontend && npm run build`

Expected: TypeScript 与 Vite 构建完成，退出码为 0。

- [x] **Step 5: 验证交互与主题**

在浅色和深色主题下打开会话右栏，确认：

- 三个控件均为 36px 高，圆角、边框、阴影和背景一致。
- “新建会话”文字与加号保持蓝色。
- 搜索展开后仍占满整行，另外两项隐藏。
- Agent 菜单仍能打开，hover 与 active 状态无跳动或裁切。

- [x] **Step 6: 提交会话工具栏改动**

```bash
git add apps/desktop/src/styles/features/chat/right-panel.css
git commit -m "fix(chat): unify session toolbar glass styling"
```

### Task 3: 最终回归验证

**Files:**
- Verify: `apps/desktop/src/styles/features/shell/header.css`
- Verify: `apps/desktop/src/styles/features/chat/right-panel.css`

**Interfaces:**
- Consumes: Task 1 与 Task 2 的 CSS 结果。
- Produces: 可交付的构建和视觉验证结论。

- [x] **Step 1: 运行最终构建**

Run: `cd frontend && npm run build`

Expected: 退出码为 0，无 TypeScript 或 Vite 错误。

- [x] **Step 2: 检查窄宽度布局**

缩窄应用窗口，确认顶部控件沿现有换行规则排列且高度仍一致；打开会话右栏，确认工具栏无横向溢出。

- [x] **Step 3: 检查改动范围**

Run: `git diff HEAD~2 -- apps/desktop/src/styles/features/shell/header.css apps/desktop/src/styles/features/chat/right-panel.css`

Expected: 仅包含约定的高度和玻璃样式调整，不包含 React 结构或交互逻辑改动。
