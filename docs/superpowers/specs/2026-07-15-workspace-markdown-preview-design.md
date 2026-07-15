# 工作空间 Markdown 预览切换

日期：2026-07-15  
状态：已实现（见 [实现计划](../plans/2026-07-15-workspace-markdown-preview.md)）

## 背景

工作空间（`WorkspacePanel`）打开文件时使用 `WorkspaceEditor`（CodeMirror）编辑源码。打开 Markdown 时只能看源码，不能快速切换渲染预览。Skills 侧的 `SkillFileViewer` 已有「预览 / 源码」分段切换可参考。

## 目标

1. 在工作空间打开 `.md` / `.markdown` 时，提供「预览 / 源码」切换。
2. 预览为只读渲染；源码为可编辑 CodeMirror（保持现有保存/撤销流程）。
3. 记住上次选择（独立 localStorage，与 Skills 互不影响）。

## 非目标

- 不在预览中 WYSIWYG 编辑。
- 不做左右/上下分栏双栏预览。
- 不抽 Skills / Workspace 共享 Viewer 组件。
- 不改文件空间（`FileSpacePanel`）侧栏预览。
- 不做 Skills 式 frontmatter 卡片。
- 本轮不为预览单独加文件大小限制。

## 决策摘要

| 项 | 选择 |
|----|------|
| 默认模式（无有效记忆时） | `source` |
| 记忆 | `localStorage`：`astro.workspace.mdPreviewMode` |
| 预览交互 | 只读 `ChatMarkdown` |
| 源码交互 | 现有可编辑 `WorkspaceEditor` |
| 控件位置 | `ws-editor-head` 右侧（文件名旁） |
| 实现策略 | 在 `WorkspacePanel` 内局部实现，复用 Skills 交互心智，不抽共享组件 |

## 行为

### 适用范围

- 仅编辑视图打开文件时。
- 仅当文件名（大小写不敏感）以 `.md` 或 `.markdown` 结尾时显示切换控件。
- 非 Markdown：行为与现网一致，不显示控件。

### 模式

| 模式 | 内容区 | 可编辑 |
|------|--------|--------|
| `source` | `WorkspaceEditor` | 是 |
| `preview` | `ChatMarkdown` 渲染当前草稿 | 否 |

### 草稿与工具栏

- 仍使用同一份 `draftContent`。
- 源码编辑后切到预览，立即反映最新草稿。
- 预览态下，若有未保存改动，保存 / 撤销 / 删除仍可用（顶栏行为不变）。

### 记忆

- Key：`astro.workspace.mdPreviewMode`
- 合法值：`"preview"` \| `"source"`
- 无效或缺失：视为 `"source"`
- 读写失败：静默忽略；会话内 state 仍可切换
- 与 Skills 的 `astro.skills.mdPreviewMode` 分离

## UI

- `ws-editor-head` 内、文件名块之后（未保存徽章附近）放分段控件：`预览` / `源码`
- `role="tablist"`，图标可用 Eye / FileCode2（与 Skills 一致）
- CSS 使用 workspace 前缀（如 `.ws-md-modes`），视觉对齐 Skills 分段，不复用 `.skills-*` 类名
- i18n：`workspace.previewMode` / `workspace.previewSource`（中/英），文案分别为「预览」「源码」与 Preview / Source

## 实现落点

| 区域 | 改动 |
|------|------|
| `WorkspacePanel.tsx` | `mdMode` state；Markdown 判定；切换控件；按模式渲染编辑器或预览 |
| `workspace.css` | 分段控件与预览滚动容器样式 |
| `messages.ts` | 中英文案 |

可选：将 `isMarkdownFilename` / `readMdMode` / `writeMdMode` 留在 panel 内，或极小 util 文件；本轮不强制抽共享。

预览容器放在 `ws-editor-wrap` 内并可滚动；空内容时空白即可，不额外错误态。

## 边界

| 情况 | 处理 |
|------|------|
| 空文件 | 预览区空白 |
| 切换到非 md 文件 | 控件消失，始终源码编辑 |
| 返回目录再打开 | 继续读 localStorage |
| 大文件 | 沿用现有打开/编辑能力 |
| localStorage 失败 | 静默忽略 |

## 验收

1. 打开 `notes.md` 可见「预览 / 源码」；打开 `a.ts` 不可见。
2. 源码编辑 → 切预览 → 渲染反映最新草稿。
3. 预览不可编辑；切回源码可继续改。
4. 有未保存时，预览态仍可保存 / 撤销。
5. 刷新页面后记住上次模式。
6. 中英文文案正常。
