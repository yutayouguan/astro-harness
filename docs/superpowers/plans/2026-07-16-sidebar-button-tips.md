# Sidebar Button Tips Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Make sidebar button tips dynamically describe the next navigation or text visibility action in Chinese and English.

**Architecture:** Keep the existing state-dependent translation key selection in `App.tsx`. Update only the corresponding locale values so `title` and `aria-label` remain synchronized without adding state or component logic.

**Tech Stack:** TypeScript, React 18, project i18n message map

## Global Constraints

- Visible navigation uses `收起导航栏` / `Collapse sidebar`.
- Hidden navigation uses `显示导航栏` / `Show sidebar`.
- Visible text uses `收起文字` / `Collapse text`.
- Hidden text uses `显示文字` / `Show text`.
- `title` and `aria-label` must use the same action semantics.

---

### Task 1: Update Sidebar Button Translations

**Files:**
- Modify: `apps/desktop/src/i18n/messages.ts:559-566`
- Modify: `apps/desktop/src/i18n/messages.ts:1742-1749`

**Interfaces:**
- Consumes: Existing `sidebar.pin`, `sidebar.unpin`, `sidebar.showLabels`, `sidebar.hideLabels` keys and their ARIA variants.
- Produces: State-aware Chinese and English action labels consumed by `App.tsx`.

- [x] **Step 1: Update Chinese translations**

Set the Chinese values to:

```ts
"sidebar.pin": "显示导航栏",
"sidebar.unpin": "收起导航栏",
"sidebar.pinAria": "显示导航栏",
"sidebar.unpinAria": "收起导航栏",
"sidebar.showLabels": "显示文字",
"sidebar.hideLabels": "收起文字",
"sidebar.showLabelsAria": "显示文字",
"sidebar.hideLabelsAria": "收起文字",
```

- [x] **Step 2: Update English translations**

Set the English values to:

```ts
"sidebar.pin": "Show sidebar",
"sidebar.unpin": "Collapse sidebar",
"sidebar.pinAria": "Show sidebar",
"sidebar.unpinAria": "Collapse sidebar",
"sidebar.showLabels": "Show text",
"sidebar.hideLabels": "Collapse text",
"sidebar.showLabelsAria": "Show text",
"sidebar.hideLabelsAria": "Collapse text",
```

- [x] **Step 3: Verify the frontend build**

Run:

```bash
cd frontend
npm run build
```

Expected: TypeScript and Vite complete with exit code 0.

- [x] **Step 4: Commit the implementation**

```bash
git add apps/desktop/src/i18n/messages.ts
git commit -m "fix(sidebar): clarify dynamic button tips"
```
