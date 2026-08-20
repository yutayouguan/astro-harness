# Agent 信息界面玻璃化美化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Agent 信息页升级为与上下文用量页一致的浅色玻璃卡片界面，并为主要信息和操作加入语义图标。

**Architecture:** 保留 `ChatAgentInfo` 的加载流程、Tauri 调用和操作回调，仅扩展展示结构和局部 CSS。使用项目已有 `lucide-react`，不新增依赖。

**Tech Stack:** React 18、TypeScript 5.6、Lucide React、CSS

## Global Constraints

- 不修改数据加载、Tauri 调用、国际化文案、业务逻辑或其他侧栏 Tab。
- 保留查看上下文、编辑记忆、复制诊断和查看技能的现有行为。
- 保持浅色玻璃风格，并兼容深色主题和窄侧栏。

---

### Task 1: 图标化 Agent 信息展示结构

**Files:**
- Modify: `apps/desktop/src/components/chat/ChatAgentInfo.tsx`

**Interfaces:**
- Consumes: 现有 Agent、记忆、日记、技能状态和操作回调
- Produces: Hero、卡片标题、操作按钮和状态视图所需的图标与 CSS 钩子

- [x] **Step 1: 引入 Lucide 图标**

从 `lucide-react` 引入 `ArrowUpRight`、`BotOff`、`Brain`、`Copy`、`Gauge`、`LoaderCircle`、`NotebookText`、`Pencil` 和 `Sparkles`。

- [x] **Step 2: 美化加载和不可用状态结构**

将纯文本状态改为 `.chat-agent-state` 容器；加载时显示旋转的 `LoaderCircle`，不可用时显示 `BotOff`。保留原文案。

- [x] **Step 3: 扩展 Hero 与卡片标题**

为 Hero 增加 `.chat-agent-identity` 和 `.chat-agent-status` 钩子；卡片标题统一为 `.chat-agent-card-title`，分别渲染 `Gauge`、`Brain`、`NotebookText`、`Sparkles`。

- [x] **Step 4: 图标化操作按钮**

查看上下文与查看技能使用 `ArrowUpRight`，编辑使用 `Pencil`，复制诊断使用 `Copy`。图标均设置 `aria-hidden`，保留按钮文本与回调。

- [x] **Step 5: 运行前端构建**

Run: `cd frontend && npm run build`

Expected: TypeScript 与 Vite 构建成功。

---

### Task 2: 实现统一玻璃卡片样式

**Files:**
- Modify: `apps/desktop/src/styles/features/chat/right-panel.css:468-603`

**Interfaces:**
- Consumes: Task 1 新增的 Agent 信息 CSS 类名
- Produces: Hero、信息卡、预览、标签、按钮和状态视图的完整视觉样式

- [x] **Step 1: 美化 Hero 和状态视图**

为 Hero 增加浅色玻璃渐变、边框、内高光和头像光晕；状态视图使用虚线玻璃卡，并为加载图标增加旋转动画及 reduced-motion 处理。

- [x] **Step 2: 美化信息卡和标题**

卡片使用与 Context Explorer 一致的半透明渐变、边框和阴影；标题图标使用柔和蓝色底座。

- [x] **Step 3: 美化预览与技能标签**

记忆和日记预览增加内层背景、边框、行距、滚动条样式与边缘渐变；技能标签使用带边框高光的玻璃胶囊。

- [x] **Step 4: 美化操作按钮与回合信息**

统一 `.linkish` 和 `.chat-agent-view-all` 的图标间距、悬停与焦点样式；回合 code 使用更清晰的胶囊样式。

- [x] **Step 5: 添加深色与窄侧栏适配**

为新增玻璃表面提供深色主题覆盖；在窄侧栏降低内边距与间距，确保标题和按钮不溢出。

- [x] **Step 6: 验证并提交**

Run: `cd frontend && npm run build`

Expected: 构建成功，相关文件无 lint 错误。

```bash
git add apps/desktop/src/components/chat/ChatAgentInfo.tsx apps/desktop/src/styles/features/chat/right-panel.css
git commit -m "style(chat): polish agent info panel"
```
