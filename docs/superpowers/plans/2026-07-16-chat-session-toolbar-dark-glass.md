# Chat Session Toolbar Dark Glass Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 让对话侧栏会话工具栏中的“新建会话”和 Agent 选择按钮在暗色主题下呈现深色毛玻璃表面。

**Architecture:** 仅在会话工具栏作用域内覆盖现有 `--session-toolbar-glass-*` CSS 变量，由现有按钮选择器继续消费。亮色主题、组件结构、尺寸和事件处理保持不变。

**Tech Stack:** CSS、React 18、TypeScript 5.6、Vite 5

## Global Constraints

- 只修改会话工具栏的暗色主题视觉，不影响其他 AgentPicker 实例。
- 保留现有蓝色强调色、按钮尺寸、圆角、间距及交互逻辑。
- 搜索按钮继续与同组控件共享一致的玻璃表面。
- 不引入新依赖或组件测试框架。

---

### Task 1: 增加会话工具栏暗色玻璃变量

**Files:**
- Modify: `apps/desktop/src/styles/features/chat/right-panel.css:169-308`

**Interfaces:**
- Consumes: `html[data-theme="dark"]`、`--tone`、`--tone-soft`、`.chat-session-toolbar` 已有局部变量接口。
- Produces: 暗色主题下的 `--session-toolbar-glass-fill`、`--session-toolbar-glass-fill-hover`、`--session-toolbar-glass-edge`、`--session-toolbar-glass-shadow`、`--session-toolbar-glass-shadow-hover`。

- [x] **Step 1: 记录修改前验证**

在应用暗色主题的界面打开“对话侧栏 → 会话”，确认“新建会话”和 Agent 选择按钮仍使用明显偏白的亮色玻璃填充；悬停或打开 Agent 菜单时边框也过亮。此项目没有 CSS 视觉测试框架，因此该现象作为手动回归基线。

- [x] **Step 2: 增加暗色变量覆盖**

在 `apps/desktop/src/styles/features/chat/right-panel.css` 的 `.chat-session-new svg` 规则之后加入：

```css
html[data-theme="dark"] .chat-session-toolbar {
  --session-toolbar-glass-fill:
    linear-gradient(155deg, rgba(255, 255, 255, 0.06) 0%, rgba(255, 255, 255, 0.015) 100%),
    color-mix(in srgb, var(--tone-soft, transparent) 28%, rgba(8, 6, 18, 0.55));
  --session-toolbar-glass-fill-hover:
    linear-gradient(155deg, rgba(255, 255, 255, 0.1) 0%, rgba(255, 255, 255, 0.03) 100%),
    color-mix(in srgb, var(--tone-soft, transparent) 42%, rgba(8, 6, 18, 0.52));
  --session-toolbar-glass-edge:
    color-mix(in srgb, var(--tone, #94a3b8) 22%, rgba(255, 255, 255, 0.08));
  --session-toolbar-glass-shadow:
    0 8px 22px rgba(0, 0, 0, 0.4),
    0 1px 3px rgba(0, 0, 0, 0.24),
    inset 0 1px 0 rgba(255, 255, 255, 0.08),
    inset 0 0 0 0.5px rgba(255, 255, 255, 0.04);
  --session-toolbar-glass-shadow-hover:
    0 12px 28px color-mix(in srgb, var(--tone, #94a3b8) 18%, rgba(0, 0, 0, 0.4)),
    inset 0 1px 0 rgba(255, 255, 255, 0.2);
}

html[data-theme="dark"] .chat-session-toolbar .expandable-search-btn:hover,
html[data-theme="dark"] .chat-session-toolbar .chat-session-new:hover,
html[data-theme="dark"] .chat-session-toolbar .agent-picker-chip:hover:not(:disabled),
html[data-theme="dark"] .chat-session-toolbar .agent-picker-chip.is-open {
  border-color: color-mix(in srgb, var(--tone, #94a3b8) 40%, rgba(255, 255, 255, 0.16));
}
```

- [x] **Step 3: 执行前端构建**

Run: `cd frontend && npm run build`

Expected: TypeScript 与 Vite 构建成功，命令退出码为 `0`。

- [x] **Step 4: 手动检查亮暗主题和交互状态**

依次检查：

1. 暗色主题默认状态：两个按钮为深紫黑半透明表面，文字与图标清晰。
2. 暗色主题 hover / Agent 菜单打开状态：表面仅轻微提亮，蓝色 tone 边框可见但不发白。
3. 亮色主题默认与 hover 状态：与修改前一致。
4. 搜索按钮：高度、圆角、边框和同组按钮一致。

- [x] **Step 5: 检查并提交**

Run: `git diff --check && git diff -- apps/desktop/src/styles/features/chat/right-panel.css`

Expected: `git diff --check` 无输出；差异只包含暗色会话工具栏变量及 hover 边框覆盖。

```bash
git add apps/desktop/src/styles/features/chat/right-panel.css
git commit -m "style(chat): adapt session toolbar to dark theme"
```
