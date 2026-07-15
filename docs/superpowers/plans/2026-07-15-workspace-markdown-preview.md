# 工作空间 Markdown 预览切换 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 工作空间打开 `.md` / `.markdown` 时，在编辑器顶栏提供「预览 / 源码」切换；预览只读渲染，源码可编辑，并记住上次模式。

**Architecture:** 抽出极小纯函数工具（判定扩展名 + localStorage 读写），`WorkspacePanel` 编辑态用分段控件切换 `WorkspaceEditor` 与 `ChatMarkdown`。样式跟 Skills 分段心智对齐，但用 `.ws-md-*` 前缀，避免耦合。

**Tech Stack:** React、现有 `ChatMarkdown` / `WorkspaceEditor`、lucide-react、localStorage、中英文 `messages.ts`、node:test。

**参考:** [设计文档](../specs/2026-07-15-workspace-markdown-preview-design.md)、`SkillFileViewer.tsx`（交互参考，不抽共享）

## Global Constraints

- localStorage key 必须为 `astro.workspace.mdPreviewMode`（与 Skills 的 `astro.skills.mdPreviewMode` 分离）。
- 无效/缺失记忆时默认 `"source"`。
- 仅 `.md` / `.markdown`（大小写不敏感）显示切换。
- 预览只读；不做分栏 / WYSIWYG / frontmatter 卡片 / FileSpace 改动 / 抽共享 Viewer。
- 草稿仍是同一份 `draftContent`；预览态下保存/撤销/删除逻辑不变。

---

## File Structure

| File | Responsibility |
|------|----------------|
| Create: `frontend/src/lib/workspaceMdMode.ts` | `isMarkdownFilename`、`readWorkspaceMdMode`、`writeWorkspaceMdMode`、`MdMode` |
| Create: `frontend/src/lib/workspaceMdMode.test.ts` | 纯函数单测 |
| Modify: `frontend/src/components/WorkspacePanel.tsx` | state、控件、按模式渲染 |
| Modify: `frontend/src/styles/workspace.css` | `.ws-md-modes` / `.ws-md-mode` / `.ws-md-preview` |
| Modify: `frontend/src/i18n/messages.ts` | `workspace.previewMode` / `workspace.previewSource` |

---

### Task 1: Markdown 模式纯函数 + 单测

**Files:**
- Create: `frontend/src/lib/workspaceMdMode.ts`
- Create: `frontend/src/lib/workspaceMdMode.test.ts`

**Interfaces:**
- Produces:
  - `export type MdMode = "preview" | "source"`
  - `export const WORKSPACE_MD_MODE_KEY = "astro.workspace.mdPreviewMode"`
  - `export function isMarkdownFilename(filename: string): boolean`
  - `export function readWorkspaceMdMode(): MdMode` — 默认 `"source"`
  - `export function writeWorkspaceMdMode(mode: MdMode): void` — 失败静默

- [ ] **Step 1: Write the failing test**

```ts
import assert from "node:assert/strict";
import test from "node:test";
import {
  WORKSPACE_MD_MODE_KEY,
  isMarkdownFilename,
  readWorkspaceMdMode,
  writeWorkspaceMdMode,
} from "./workspaceMdMode.ts";

test("isMarkdownFilename accepts md and markdown case-insensitively", () => {
  assert.equal(isMarkdownFilename("notes.md"), true);
  assert.equal(isMarkdownFilename("NOTES.MD"), true);
  assert.equal(isMarkdownFilename("doc.markdown"), true);
  assert.equal(isMarkdownFilename("a.ts"), false);
  assert.equal(isMarkdownFilename("readme.mdx"), false);
});

test("readWorkspaceMdMode defaults to source and accepts stored values", () => {
  const g = globalThis as { localStorage?: Storage };
  const store = new Map<string, string>();
  g.localStorage = {
    getItem: (k) => store.get(k) ?? null,
    setItem: (k, v) => {
      store.set(k, v);
    },
    removeItem: (k) => {
      store.delete(k);
    },
    clear: () => store.clear(),
    key: () => null,
    length: 0,
  };
  store.clear();
  assert.equal(readWorkspaceMdMode(), "source");
  store.set(WORKSPACE_MD_MODE_KEY, "preview");
  assert.equal(readWorkspaceMdMode(), "preview");
  store.set(WORKSPACE_MD_MODE_KEY, "source");
  assert.equal(readWorkspaceMdMode(), "source");
  store.set(WORKSPACE_MD_MODE_KEY, "nope");
  assert.equal(readWorkspaceMdMode(), "source");
  writeWorkspaceMdMode("preview");
  assert.equal(store.get(WORKSPACE_MD_MODE_KEY), "preview");
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && node --test src/lib/workspaceMdMode.test.ts`  
Expected: FAIL（模块不存在）

- [ ] **Step 3: Write minimal implementation**

```ts
/** 工作空间 Markdown 预览/源码模式持久化与判定。 */

export type MdMode = "preview" | "source";

export const WORKSPACE_MD_MODE_KEY = "astro.workspace.mdPreviewMode";

/** 是否为可切换预览的 Markdown 文件名 */
export function isMarkdownFilename(filename: string): boolean {
  const lower = filename.toLowerCase();
  return lower.endsWith(".md") || lower.endsWith(".markdown");
}

/** 读取上次模式；无效或不可读时默认 source */
export function readWorkspaceMdMode(): MdMode {
  try {
    const v = localStorage.getItem(WORKSPACE_MD_MODE_KEY);
    if (v === "source" || v === "preview") return v;
  } catch {
    // ignore
  }
  return "source";
}

/** 写入模式；失败静默 */
export function writeWorkspaceMdMode(mode: MdMode): void {
  try {
    localStorage.setItem(WORKSPACE_MD_MODE_KEY, mode);
  } catch {
    // ignore
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && node --test src/lib/workspaceMdMode.test.ts`  
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/workspaceMdMode.ts frontend/src/lib/workspaceMdMode.test.ts
git commit -m "$(cat <<'EOF'
feat(workspace): add markdown preview mode helpers

EOF
)"
```

---

### Task 2: i18n 文案

**Files:**
- Modify: `frontend/src/i18n/messages.ts`

**Interfaces:**
- Produces keys: `workspace.previewMode` / `workspace.previewSource`

- [ ] **Step 1: Add Chinese strings**（紧挨 `workspace.openExternally` 附近）

```ts
"workspace.previewMode": "预览",
"workspace.previewSource": "源码",
```

- [ ] **Step 2: Add English strings**（英文区块对应位置）

```ts
"workspace.previewMode": "Preview",
"workspace.previewSource": "Source",
```

- [ ] **Step 3: Commit**

```bash
git add frontend/src/i18n/messages.ts
git commit -m "$(cat <<'EOF'
i18n(workspace): add markdown preview mode labels

EOF
)"
```

---

### Task 3: WorkspacePanel 切换 UI + 渲染

**Files:**
- Modify: `frontend/src/components/WorkspacePanel.tsx`
- Modify: `frontend/src/styles/workspace.css`

**Interfaces:**
- Consumes: `isMarkdownFilename`, `readWorkspaceMdMode`, `writeWorkspaceMdMode`, `MdMode` from `../lib/workspaceMdMode`
- Consumes: `ChatMarkdown` from `./ChatMarkdown`
- Consumes: lucide `Eye`, `FileCode2`

- [ ] **Step 1: Imports + state**（在 `WorkspacePanel` 组件内，与其它 useState 并列）

```tsx
import { Eye, FileCode2 } from "lucide-react";
import { ChatMarkdown } from "./ChatMarkdown";
import {
  isMarkdownFilename,
  readWorkspaceMdMode,
  writeWorkspaceMdMode,
  type MdMode,
} from "../lib/workspaceMdMode";

// inside component:
const [mdMode, setMdMode] = useState<MdMode>(() => readWorkspaceMdMode());

const setMdModePersist = (mode: MdMode) => {
  setMdMode(mode);
  writeWorkspaceMdMode(mode);
};

const editorIsMarkdown = isMarkdownFilename(editorName);
const showMdPreview = editorIsMarkdown && mdMode === "preview";
```

注意：`editorName` 已在编辑态存在；若此时变量作用域仅在分支内，把 `editorIsMarkdown` / `showMdPreview` 放在使用它们的 JSX 附近（编辑视图分支内）亦可。

- [ ] **Step 2: 在 `ws-editor-head` 加入分段控件**

放在 meta 块之后、dirty badge 之前（或 badge 之后；badge 用 `flex-shrink: 0`，modes 也要 `flex-shrink: 0`）：

```tsx
<div className="ws-editor-head">
  <FileGlyph name={editorName} isDir={false} />
  <div className="ws-editor-meta-block">
    <h3 className="ws-editor-filename">{editorName}</h3>
    <p className="ws-editor-path">{editorPath}</p>
  </div>
  {editorIsMarkdown && (
    <div className="ws-md-modes" role="tablist" aria-label={t("workspace.previewMode")}>
      <button
        type="button"
        role="tab"
        aria-selected={mdMode === "preview"}
        className={`ws-md-mode ${mdMode === "preview" ? "is-active" : ""}`}
        onClick={() => setMdModePersist("preview")}
      >
        <Eye size={13} strokeWidth={2.3} aria-hidden />
        {t("workspace.previewMode")}
      </button>
      <button
        type="button"
        role="tab"
        aria-selected={mdMode === "source"}
        className={`ws-md-mode ${mdMode === "source" ? "is-active" : ""}`}
        onClick={() => setMdModePersist("source")}
      >
        <FileCode2 size={13} strokeWidth={2.3} aria-hidden />
        {t("workspace.previewSource")}
      </button>
    </div>
  )}
  {dirty && <span className="ws-dirty-badge">{t("workspace.unsaved")}</span>}
</div>
```

- [ ] **Step 3: 按模式渲染 `ws-editor-wrap`**

```tsx
<div className="ws-editor-wrap">
  {showMdPreview ? (
    <div className="ws-md-preview">
      <ChatMarkdown content={draftContent} />
    </div>
  ) : (
    <WorkspaceEditor
      value={draftContent}
      filename={editorName}
      theme={theme}
      onChange={setDraftContent}
    />
  )}
</div>
```

- [ ] **Step 4: CSS**（追加到 `workspace.css`，建议放在 `.ws-editor-head` 相关规则附近）

```css
.ws-md-modes {
  display: inline-flex;
  gap: 4px;
  flex-shrink: 0;
  padding: 3px;
  border-radius: 12px;
  border: 1px solid color-mix(in srgb, var(--glass-edge) 80%, transparent);
  background: color-mix(in srgb, var(--chip-bg) 70%, transparent);
}

.ws-md-mode {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 5px 10px;
  border: none;
  border-radius: 9px;
  font-size: 12px;
  font-weight: 650;
  color: var(--ink-mute);
  background: transparent;
  cursor: pointer;
}

.ws-md-mode.is-active {
  color: var(--tone);
  background:
    linear-gradient(165deg, rgba(255, 255, 255, 0.8), rgba(255, 255, 255, 0.4)),
    var(--tone-soft);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.85);
}

html[data-theme="dark"] .ws-md-mode.is-active {
  background: color-mix(in srgb, var(--tone) 16%, rgba(255, 255, 255, 0.06));
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--tone) 28%, transparent);
  color: color-mix(in srgb, #fff 18%, var(--tone));
}

.ws-md-preview {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 14px 16px;
}
```

窄屏时：`ws-editor-head` 已有 `align-items: center`；若溢出，可为 head 加 `flex-wrap: wrap`（仅当实测换行需要时再加，YAGNI）。

- [ ] **Step 5: Typecheck**

Run: `cd frontend && npx tsc -b --pretty false`  
Expected: 无新增错误

- [ ] **Step 6: 手动验收（对照 spec）**

1. 打开 `notes.md` → 见「预览 / 源码」；`a.ts` → 不见  
2. 源码改字 → 预览更新  
3. 预览不可输入；切回源码可改  
4. dirty 时预览态仍可保存/撤销  
5. 刷新后模式保持  
6. 切中英文文案正确  

- [ ] **Step 7: Commit**

```bash
git add frontend/src/components/WorkspacePanel.tsx frontend/src/styles/workspace.css
git commit -m "$(cat <<'EOF'
feat(workspace): toggle markdown preview and source

EOF
)"
```

---

### Task 4: Spec 状态收口

**Files:**
- Modify: `docs/superpowers/specs/2026-07-15-workspace-markdown-preview-design.md`

- [ ] **Step 1:** 将状态改为「已实现」，并链到本 plan。

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/specs/2026-07-15-workspace-markdown-preview-design.md
git commit -m "$(cat <<'EOF'
docs: mark workspace markdown preview design implemented

EOF
)"
```

---

## Spec coverage (self-review)

| Spec 要求 | Task |
|-----------|------|
| `.md` / `.markdown` 才显示 | T1 + T3 |
| 预览 ChatMarkdown 只读 / 源码可编辑 | T3 |
| localStorage key + 默认 source | T1 |
| 控件在 editor-head | T3 |
| i18n | T2 |
| 草稿/保存不变 | T3（不改 save/undo 路径） |
| 无 frontmatter / 不分栏 / 不改 FileSpace | 遵守非目标 |
| 验收条目 | T3 Step 6 |
