# Provider Detail Tab Icons Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为模型提供商详情页的“聊天”和“媒体”Tab 增加与现有界面一致的 Lucide 线性图标。

**Architecture:** 在现有 `ProvidersPanel` 中直接复用项目已安装的 `lucide-react`，不引入新组件或依赖。通过 Tab 按钮自身的 Flex 布局完成图标与文字对齐，让 SVG 使用 `currentColor` 自动继承现有选中态和未选中态颜色。

**Tech Stack:** React 18、TypeScript 5.6、lucide-react、CSS、Vite

## Global Constraints

- “聊天”使用 `MessageCircle`，“媒体”使用 `Image`。
- 图标尺寸为 14px，`strokeWidth` 为 2，位于文字左侧并保持 6px 间距。
- 图标设置 `aria-hidden`，Tab 的可访问名称继续由文字提供。
- 不改变 Tab 状态、点击逻辑、文案、下划线、面板内容或响应式行为。

---

### Task 1: 为提供商详情 Tab 添加图标

**Files:**
- Modify: `apps/desktop/src/components/settings/ProvidersPanel.tsx:12-33,1375-1395`
- Modify: `apps/desktop/src/styles/features/providers.css:377-400`

**Interfaces:**
- Consumes: `lucide-react` 导出的 `MessageCircle` 与 `Image` React 图标组件；现有 `detailTab: "chat" | "media"` 状态。
- Produces: 两个保持原有点击和可访问行为、带装饰性 SVG 图标的 `.providers-detail-tab` 按钮。

- [x] **Step 1: 运行基线构建**

Run:

```bash
cd frontend
npm run build
```

Expected: TypeScript 编译和 Vite 构建均成功，命令退出码为 0。

- [x] **Step 2: 添加 Lucide 图标引用和 Tab 图标**

在 `apps/desktop/src/components/settings/ProvidersPanel.tsx` 的 `lucide-react` 导入列表中加入：

```tsx
Image,
MessageCircle,
```

将两个 Tab 按钮内容改为：

```tsx
<MessageCircle size={14} strokeWidth={2} aria-hidden />
{t("providers.tabChat")}
```

```tsx
<Image size={14} strokeWidth={2} aria-hidden />
{t("providers.tabMedia")}
```

保留按钮现有的 `type`、`role`、`aria-selected`、`className`、`onClick` 和翻译键。

- [x] **Step 3: 对齐图标与文字**

在 `apps/desktop/src/styles/features/providers.css` 的 `.providers-detail-tab` 中加入：

```css
display: inline-flex;
align-items: center;
justify-content: center;
gap: 6px;
```

不修改现有 padding、字体、颜色、边框与选中态规则。Lucide SVG 默认使用 `currentColor`，因此不增加独立图标颜色规则。

- [x] **Step 4: 运行构建验证**

Run:

```bash
cd frontend
npm run build
```

Expected: TypeScript 编译无未使用导入或 JSX 类型错误；Vite 构建成功，命令退出码为 0。

- [x] **Step 5: 检查编辑文件诊断**

检查以下文件的 IDE linter：

```text
apps/desktop/src/components/settings/ProvidersPanel.tsx
apps/desktop/src/styles/features/providers.css
```

Expected: 两个文件均无新增诊断。

- [x] **Step 6: 提交实现**

```bash
git add apps/desktop/src/components/settings/ProvidersPanel.tsx apps/desktop/src/styles/features/providers.css
git commit -m "feat(providers): add icons to detail tabs"
git status --short
```

Expected: 创建一个仅包含上述两个实现文件的提交；工作区中用户原有的无关改动保持不变。
