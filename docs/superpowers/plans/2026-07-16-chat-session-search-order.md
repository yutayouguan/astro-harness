# 会话工具栏搜索按钮顺序 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将对话侧栏会话工具栏的搜索按钮移到最右侧，使视觉顺序和键盘焦点顺序均为“新建会话 → Agent → 搜索”。

**Architecture:** 仅调整 `ChatSessionList` 工具栏子元素的 JSX/DOM 顺序，不引入状态、样式或行为变化。由于前端当前没有组件测试框架，通过 TypeScript/Vite 构建和界面手动检查验证。

**Tech Stack:** React 18、TypeScript 5.6、Vite 5

## Global Constraints

- 不修改控件样式、尺寸、搜索逻辑和响应式行为。
- DOM 顺序必须与视觉顺序、键盘 Tab 顺序一致。

---

### Task 1: 调整会话工具栏控件顺序

**Files:**
- Modify: `apps/desktop/src/components/chat/ChatSessionList.tsx:101-124`

**Interfaces:**
- Consumes: `query`、`setQuery`、`onNewSession`、`agents`、`activeAgentId`、`handleAgentChange`、`onNewAgent`
- Produces: DOM 顺序为 `.chat-session-new`、`.chat-session-agent-picker`、`.chat-session-search`

- [ ] **Step 1: 调整 JSX 顺序**

将 `ExpandableSearch` 从工具栏首位移动到 `AgentPicker` 后，保留所有属性不变：

```tsx
<div className="chat-session-toolbar">
  <button
    type="button"
    className="chat-session-new"
    onClick={onNewSession}
  >
    <Plus size={15} strokeWidth={2.2} aria-hidden />
    {t("chat.newSession")}
  </button>
  <AgentPicker
    agents={agents}
    value={activeAgentId}
    onChange={(id) => void handleAgentChange(id)}
    onCreateNew={onNewAgent}
    labelKey="chat.rightPanel.agent"
    className="chat-session-agent-picker"
  />
  <ExpandableSearch
    value={query}
    onChange={setQuery}
    placeholderKey="chat.rightPanel.searchSessions"
    className="chat-session-search"
  />
</div>
```

- [ ] **Step 2: 运行前端构建**

Run: `cd frontend && npm run build`

Expected: TypeScript 编译和 Vite 构建成功，命令退出码为 `0`。

- [ ] **Step 3: 手动验证交互与焦点顺序**

启动应用并打开对话侧栏，确认：

1. 工具栏从左到右为“新建会话 → Agent → 搜索”。
2. 点击搜索按钮后输入关键词，会话列表仍正确过滤。
3. 键盘 Tab 焦点依次经过新建会话、Agent 和搜索。
4. 搜索展开时没有遮挡、溢出或布局跳动异常。

- [ ] **Step 4: 提交实现**

```bash
git add apps/desktop/src/components/chat/ChatSessionList.tsx
git commit -m "style(chat): move session search to toolbar end"
```
