# 文件空间多类型查看与文本编辑 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 文件空间右侧支持文本可编辑（自动+手动保存）、MD/HTML 预览↔源码、图片/视频/音频/PDF 内嵌查看。

**Architecture:** 新建 `FileSpaceViewer` 接管右侧内容；复用 `WorkspaceEditor`、`ChatMarkdown`、已有 `MediaPreview`；`fileTypeIcon` 增加 `media-pdf`；`FileSpacePanel` 只保留列表与头栏操作。

**Tech Stack:** React + Tauri `read_file` / `write_file` / `convertFileSrc`；现有 CodeMirror / ChatMarkdown / MediaPreview。

**参考:** [设计规格](../specs/2026-07-15-filespace-media-viewer-design.md)

## Global Constraints

- 复用已有 `apps/desktop/src/components/media/*`，不重复造图/音/视/HTML iframe。
- Office（docx/xlsx/pptx）仍 `external`；本期只把 **pdf** 改为 `media-pdf`。
- HTML sandbox：`allow-scripts`（与现有 `HtmlPreview` 一致）。
- 自动保存防抖约 600ms；切换文件前 flush。
- 不改 Workspace 保存策略；不引入 pdf.js（除非资产协议失败再评估）。

---

## File Structure

| File | Responsibility |
|------|----------------|
| Modify: `apps/desktop/src/lib/fileTypeIcon.ts` | `media-pdf`；`mediaKindOf` 可返回 `pdf` 或单独 helper |
| Modify: `apps/desktop/src/lib/fileTypeIcon.test.ts` | pdf / 音频回归 |
| Create: `apps/desktop/src/lib/filespaceViewerKind.ts` (+test) | 纯函数：artifact → viewer kind |
| Create: `apps/desktop/src/components/FileSpaceViewer.tsx` | 右侧内容：各 pane + 保存 |
| Modify: `apps/desktop/src/components/FileSpacePanel.tsx` | 删 PreviewState / TEXT_EXTS；挂 Viewer |
| Modify: `apps/desktop/src/styles/filespace.css` | viewer 编辑/分栏样式 |
| Modify: `apps/desktop/src/i18n/messages.ts` | 未保存/保存中等（可复用 workspace keys） |

---

### Task 1: `media-pdf` + viewer kind 纯函数

**Files:** `fileTypeIcon.ts`, `fileTypeIcon.test.ts`, create `filespaceViewerKind.ts` + test

- [x] **Step 1:** 扩展 `FileOpenMode` 增加 `"media-pdf"`；`pdf` 条目改为 `media-pdf`（不再 `external`）。

```ts
pdf: entry("pdf", ScrollText, "media-pdf"),
```

- [x] **Step 2:** `mediaKindOf` 增加 pdf 分支，或新增：

```ts
export function isPdfFile(name: string): boolean {
  return resolveFileType(name, false).open === "media-pdf";
}
```

选用 `isPdfFile`（避免把 pdf 塞进 `GeneratedMediaKind`）。

- [x] **Step 3:** 新建 `filespaceViewerKind.ts`：

```ts
export type FilespaceViewerKind =
  | "markdown"
  | "html"
  | "text"
  | "image"
  | "video"
  | "audio"
  | "pdf"
  | "external"
  | "missing";

export function filespaceViewerKind(input: {
  name: string;
  missing?: boolean;
  mime?: string | null;
  category?: string | null;
}): FilespaceViewerKind;
```

规则：`missing` → missing；`mediaKindOf` → image/video/audio/html；`isPdfFile` → pdf；扩展名 md/markdown/mdx → markdown；`open === "text"` 或 mime `text/*` / json / category code|doc|sheet → text；否则 external。

- [x] **Step 4:** 测试覆盖 md/html/png/mp4/mp3/pdf/docx/missing。

```bash
cd frontend && node --test src/lib/fileTypeIcon.test.ts src/lib/filespaceViewerKind.test.ts
```

- [x] **Step 5:** Commit `feat(filespace): add media-pdf and viewer kind resolver`

---

### Task 2: `FileSpaceViewer` — 文本编辑 + 自动/手动保存

**Files:** Create `FileSpaceViewer.tsx`

**Props:**

```ts
type Props = {
  path: string;
  name: string;
  missing?: boolean;
  mime?: string | null;
  category?: string | null;
  onOpenExternally: () => void;
};
```

- [x] **Step 1:** kind=`text`：`read_file` → `WorkspaceEditor`；draft / lastSaved；脏徽章复用 `t("workspace.unsaved")`；保存按钮 `t("workspace.save")`。
- [x] **Step 2:** 防抖 600ms `write_file`；⌘S/Ctrl+S 立即写；保存失败设 error 字符串，不更新 lastSaved。
- [x] **Step 3:** `path` 变化时：先 await flush 待写；失败 `window.confirm(t("workspace.unsavedConfirm"))` 后丢弃或重试。
- [x] **Step 4:** 手工/tsc 无类型错；commit `feat(filespace): editable text viewer with autosave`

---

### Task 3: Markdown / HTML 双模式

- [x] **Step 1:** kind=`markdown`：工具栏预览/源码（文案 `workspace.previewMode` / `previewSource`）。预览：`ChatMarkdown`；源码：Editor + 下方/旁侧 `ChatMarkdown(draft)` 实时预览。
- [x] **Step 2:** kind=`html`：预览态 `HtmlPreview`/`MediaPreview kind=html`；源码态 Editor + `HtmlPreview source={draft}` 实时。
- [x] **Step 3:** 与 Task 2 共用保存逻辑（抽 `useFileDraft(path)` 或内联 helper）。
- [x] **Step 4:** Commit `feat(filespace): markdown and html preview/source modes`

---

### Task 4: 媒体 + PDF pane

- [x] **Step 1:** image/video/audio → 现有 `MediaPreview`。
- [x] **Step 2:** pdf → `convertFileSrc`/`resolveMediaSrc` + `<iframe className="fs-preview-pdf">`；`onError` / 非 Tauri → BrokenMedia 式提示 + `onOpenExternally`。
- [x] **Step 3:** external/missing → 现有文案 + 打开按钮。
- [x] **Step 4:** Commit `feat(filespace): embed media and pdf in viewer`

---

### Task 5: 接入 Panel + 样式/i18n

- [x] **Step 1:** `FileSpacePanel` 删除 `PreviewState`、`TEXT_EXTS`、`IMAGE_EXTS`、预览 `useEffect`；aside body 改为：

```tsx
<FileSpaceViewer
  path={selected.path}
  name={selected.name}
  missing={selected.missing}
  mime={selected.mime}
  category={selected.category}
  onOpenExternally={() => void openSelectedExternally()}
/>
```

头栏操作按钮保留在 Panel。

- [x] **Step 2:** `filespace.css`：`.fs-viewer`、`.fs-viewer-toolbar`、`.fs-viewer-split`、`.fs-preview-pdf`、脏徽章对齐 workspace。
- [x] **Step 3:** 需要时增加 `filespace.saving`；其余复用 workspace keys。
- [x] **Step 4:** Commit `feat(filespace): wire FileSpaceViewer into panel`

---

### Task 6: 验收

- [x] `node --test` 相关单测；`npx tsc --noEmit`（frontend）
- [x] 手工：文本保存、MD/HTML 切换、图/音/视/PDF（Tauri）
- [x] 更新 design spec 状态为「实现中/已实现」
- [x] Commit `docs: mark filespace media viewer plan done`

---

## Out of scope

- Word / Excel / PPT 内嵌
- pdf.js
- Workspace 自动保存对齐
