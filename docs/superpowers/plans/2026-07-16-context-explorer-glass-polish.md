# 上下文用量界面玻璃化美化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将上下文用量页面升级为带语义图标、指标玻璃卡、强化环图和图标化空状态的浅色玻璃仪表盘。

**Architecture:** 保留 `ContextExplorer` 的数据计算和展开状态，只扩展展示结构。使用项目已有 `lucide-react` 图标，通过局部 CSS 完成视觉层级，避免新增依赖或影响其他侧栏 Tab。

**Tech Stack:** React 18、TypeScript 5.6、Lucide React、CSS

## Global Constraints

- 不修改上下文数据结构、计算函数、国际化文案或其他侧栏 Tab。
- 保持现有浅色玻璃风格，并兼容深色主题。
- 保留分层展开、收起、token 数值和提示文案行为。

---

### Task 1: 图标化上下文用量展示结构

**Files:**
- Modify: `frontend/src/components/chat/ContextExplorer.tsx`

**Interfaces:**
- Consumes: `ContextUsageSnapshot`、`SEGMENT_TONE`、现有国际化键
- Produces: 带 `.ctx-explorer-metric-icon`、`.ctx-donut-caption`、`.ctx-explorer-title-icon`、`.ctx-explorer-row-icon` 和 `.ctx-explorer-empty-icon` 的展示结构

- [ ] **Step 1: 引入指标与分层图标**

从 `lucide-react` 引入：

```tsx
import {
  Bot,
  Brain,
  ChevronDown,
  Coins,
  Database,
  Layers3,
  MessageSquare,
  MessagesSquare,
  Plug,
  Search,
  Shield,
  Sparkles,
  Wrench,
  type LucideIcon,
} from "lucide-react";
```

新增分层图标映射：

```tsx
const SEG_ICON: Record<string, LucideIcon> = {
  system: Shield,
  tools: Wrench,
  mcp: Plug,
  memory: Brain,
  skills: Sparkles,
  recall: Search,
  subagent: Bot,
  conversation: MessagesSquare,
};
```

- [ ] **Step 2: 将三个指标改为带图标的卡片**

每个 `.ctx-explorer-metric` 内加入图标容器，分别渲染 `MessageSquare`、`Database` 和 `Coins`；保留原标签和值。

- [ ] **Step 3: 强化环图和分层标题语义**

在 `.ctx-donut-center` 的百分比下方渲染现有 `chat.contextExplorer.tokensUsed` 文案；给分层标题加入 `Layers3` 图标。

- [ ] **Step 4: 给分层行和空状态加入图标**

在分层循环中通过 `const SegmentIcon = SEG_ICON[s.id] ?? Layers3` 渲染 `.ctx-explorer-row-icon`。空状态改为容器，使用 `Layers3` 图标和原有空状态文案。

- [ ] **Step 5: 运行 TypeScript 构建检查**

Run: `cd frontend && npm run build`

Expected: TypeScript 编译与 Vite 构建成功，退出码为 `0`。

---

### Task 2: 完成浅色玻璃仪表盘样式

**Files:**
- Modify: `frontend/src/styles/features/chat/right-panel.css:622-837`

**Interfaces:**
- Consumes: Task 1 新增的上下文界面类名
- Produces: 三列指标卡、环图玻璃容器、图标化列表和空状态

- [ ] **Step 1: 美化指标卡**

为 `.ctx-explorer-metric` 增加半透明渐变背景、细边框、内高光、圆角和紧凑内边距；为图标添加柔和蓝色底和统一尺寸。

- [ ] **Step 2: 美化环图容器**

将 `.ctx-donut-wrap` 扩展为带玻璃背景和柔和径向高光的容器；让中心内容纵向排列，并设置百分比与说明标签层级。

- [ ] **Step 3: 美化标题、列表行和空状态**

为分层标题和行图标建立统一尺寸与颜色；让列表行具有细边框和悬停背景；将空状态改为居中虚线玻璃卡。

- [ ] **Step 4: 添加深色主题与窄宽度适配**

深色主题降低白色高光并增强边框可见度；窄侧栏下保持三列指标卡，但缩小间距和内边距，避免文字溢出。

- [ ] **Step 5: 运行上下文用量测试和前端构建**

Run:

```bash
cd frontend
node --experimental-strip-types --test src/lib/chat/contextUsage.test.ts
npm run build
```

Expected: 5 项测试全部通过，TypeScript/Vite 构建成功。

- [ ] **Step 6: 提交实现**

```bash
git add frontend/src/components/chat/ContextExplorer.tsx frontend/src/styles/features/chat/right-panel.css
git commit -m "style(chat): polish context explorer"
```
