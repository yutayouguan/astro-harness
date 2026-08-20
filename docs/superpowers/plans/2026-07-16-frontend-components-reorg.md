# Frontend Components Domain Reorg Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Move `apps/desktop/src/components/` from a flat layout into domain folders without changing runtime behavior or UI.

**Architecture:** `git mv` files into domain dirs (`chat/`, `filespace/`, `workspace/`, `agents/`, `schedule/`, `settings/`, `ui/`, `icons/`; keep `media/`). Update relative imports (`../X` → `../../X` for nested files) and all consumers (`App.tsx`, hooks, lib, a2ui, sibling components). Optional per-domain `index.ts` barrels. Verify with `npm run build` (runs `tsc -b && vite build`).

**Tech Stack:** React + TypeScript + Vite (`apps/desktop/`), existing relative imports (no new path-alias scheme).

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-16-frontend-components-reorg-design.md`
- No behavior / UI changes — move + import fix only
- No permanent root-level re-export shims
- Do not move `hooks/`, `lib/`, `contexts/`, `a2ui/`; do not split `App.tsx`
- Prefer `git mv` to preserve history
- Commit once per domain task after `npm run build` passes
- Work on a feature branch / worktree if current `main` has unrelated WIP

## File map (locked)

| Target | Files to move from `components/` |
|--------|----------------------------------|
| `icons/` | `NavIcons.tsx`, `ProviderIcons.tsx`, `ToolIcons.tsx`, `GlassSolidIcons.tsx`, `McpIcon.tsx`, `LucideByName.tsx` |
| `ui/` | `Toast.tsx`, `SelectMenu.tsx`, `AnimatedSwitch.tsx`, `ExpandableSearch.tsx` |
| `chat/` | `ChatView.tsx`, `ChatWelcome.tsx`, `ChatSessionList.tsx`, `ChatRightPanel.tsx`, `ChatMessageNav.tsx`, `ChatMarkdown.tsx`, `ChatContextTimeline.tsx`, `ChatAgentInfo.tsx`, `MsgTimeline.tsx`, `MsgStreamLoader.tsx`, `MsgReasoning.tsx`, `MsgDissolveOverlay.tsx`, `MsgActivity.tsx`, `ComposerPalette.tsx`, `ComposerMcpMenu.tsx`, `ContextExplorer.tsx`, `ContextUsageBar.tsx`, `ContextUsagePopover.tsx`, `A2UISurfaceCard.tsx` |
| `filespace/` | `FileSpacePanel.tsx`, `FileSpaceViewer.tsx`, `FileSpaceBatchBar.tsx`, `FileSpaceConfirm.tsx`, `FileContextMenu.tsx` |
| `workspace/` | `WorkspacePanel.tsx`, `WorkspaceEditor.tsx`, `WorkspaceBatchBar.tsx`, `WorkspaceIcons.tsx` |
| `agents/` | `AgentPicker.tsx`, `AgentAvatar.tsx`, `AgentCreateGuide.tsx`, `AvatarPickerDrawer.tsx`, `ModelPicker.tsx`, `ModelCapabilityIcons.tsx`, `LucideIconPicker.tsx` |
| `schedule/` | `CronPanel.tsx`, `CreateCronDialog.tsx`, `ScheduleEditor.tsx` |
| `settings/` | `PreferencesPanel.tsx`, `ProvidersPanel.tsx`, `ToolsPanel.tsx`, `SkillsPanel.tsx`, `SkillFileViewer.tsx`, `MemoryPanel.tsx`, `InsightsPanel.tsx`, `SidebarContextMenu.tsx` |
| `media/` | unchanged |

### Import rewrite rules

After a file moves from `components/Foo.tsx` → `components/<domain>/Foo.tsx`:

1. Inside that file, any `from "../hooks|i18n|lib|types|contexts|illustrations|a2ui|..."` becomes `from "../../..."`.
2. Sibling imports that remain in the same new domain stay `./Other`.
3. Imports of files that already live in another domain (or will) use `../<other-domain>/Other` (or `./media/...` style).
4. External consumers update path, e.g. `./components/Toast` → `./components/ui/Toast`.
5. Optional: add `components/<domain>/index.ts` re-exporting public modules; consumers may use either barrel or deep path.

### Known external consumers (update as domains move)

- `apps/desktop/src/App.tsx` — many panels, NavIcons, Toast, MsgDissolveOverlay, ModelPicker, SidebarContextMenu
- `apps/desktop/src/hooks/useTransientToast.ts` — Toast
- `apps/desktop/src/hooks/useAgentTools.ts` — ToolIcons
- `apps/desktop/src/lib/workspaceMenuItems.ts` — FileContextMenu
- `apps/desktop/src/a2ui/CatalogAdapter.tsx` — `media/MediaPreview` (no change for media path)

Plus dense cross-imports among components (especially `ChatView` ↔ Msg*/Composer*/Context*/media).

### Verification command (every task)

```bash
cd frontend && npm run build
```

Expected: `tsc -b` and `vite build` succeed with exit code 0.

---

### Task 1: Branch + icons domain

**Files:**
- Move: listed `icons/` files
- Create: `apps/desktop/src/components/icons/index.ts`
- Modify: all importers of those icons (search `components/NavIcons`, `ProviderIcons`, `ToolIcons`, `GlassSolidIcons`, `McpIcon`, `LucideByName`)

**Interfaces:**
- Produces: `components/icons/*` and barrel exports matching prior named/default exports

- [x] **Step 1: Create branch** (or worktree) from current HEAD; leave unrelated WIP unstaged

```bash
git checkout -b chore/frontend-components-reorg
```

- [x] **Step 2: Move icon files**

```bash
cd apps/desktop/src/components
mkdir -p icons
git mv NavIcons.tsx ProviderIcons.tsx ToolIcons.tsx GlassSolidIcons.tsx McpIcon.tsx LucideByName.tsx icons/
```

- [x] **Step 3: Fix imports inside moved files** (`../` → `../../` for src-level modules; keep `@lobehub/...` unchanged)

- [x] **Step 4: Update consumers** (at least `App.tsx`, `useAgentTools.ts`, and any component still importing old paths)

- [x] **Step 5: Add barrel**

```ts
// apps/desktop/src/components/icons/index.ts
export * from "./NavIcons";
export { default as ProviderIcons } from "./ProviderIcons";
// ... re-export each module’s public API to match prior usage
```

Use the actual export style of each file (default vs named); mirror callers.

- [x] **Step 6: Verify**

```bash
cd frontend && npm run build
```

- [x] **Step 7: Commit**

```bash
git add apps/desktop/src/components/icons apps/desktop/src/App.tsx apps/desktop/src/hooks apps/desktop/src/components
git commit -m "$(cat <<'EOF'
refactor(ui): move icon components into components/icons

EOF
)"
```

---

### Task 2: ui domain

**Files:**
- Move: `Toast.tsx`, `SelectMenu.tsx`, `AnimatedSwitch.tsx`, `ExpandableSearch.tsx`
- Create: `apps/desktop/src/components/ui/index.ts`
- Modify: importers (`App.tsx`, `useTransientToast.ts`, components that import SelectMenu/ExpandableSearch/Toast)

- [x] **Step 1: git mv into `ui/`**
- [x] **Step 2: Fix nested relative imports (`../` → `../../`)**
- [x] **Step 3: Update all consumers to `components/ui/...`**
- [x] **Step 4: Add `ui/index.ts` barrel**
- [x] **Step 5: `cd frontend && npm run build`**
- [x] **Step 6: Commit** `refactor(ui): move shared controls into components/ui`

---

### Task 3: chat domain

**Files:**
- Move: all `chat/` files from File map
- Create: `apps/desktop/src/components/chat/index.ts`
- Modify: `App.tsx` + every cross-import among chat files and into `media/`, `agents/`, `ui/`, `icons/` as already moved

**Interfaces:**
- Produces: chat components under `components/chat/`; `MSG_DISSOLVE_MS` still exported from `MsgDissolveOverlay`

- [x] **Step 1: git mv chat cluster into `chat/`**
- [x] **Step 2: Fix `../` → `../../` for src modules; keep `./media/...` as `../media/...`**
- [x] **Step 3: Update sibling imports that now live in other domains (icons/ui already moved)**
- [x] **Step 4: Update `App.tsx` paths for Chat*, MsgDissolveOverlay**
- [x] **Step 5: Add `chat/index.ts` barrel for publicly imported chat modules**
- [x] **Step 6: `cd frontend && npm run build`**
- [x] **Step 7: Commit** `refactor(ui): move chat components into components/chat`

---

### Task 4: filespace domain

**Files:**
- Move: filespace cluster
- Create: `apps/desktop/src/components/filespace/index.ts`
- Modify: `App.tsx`, `lib/workspaceMenuItems.ts`, any chat/workspace importers

- [x] **Step 1: git mv → `filespace/`**
- [x] **Step 2: Fix relative imports**
- [x] **Step 3: Update consumers (`FileSpacePanel`, `FileContextMenu` type import)**
- [x] **Step 4: Barrel + build + commit** `refactor(ui): move filespace components into components/filespace`

---

### Task 5: workspace domain

**Files:**
- Move: workspace cluster
- Create: `apps/desktop/src/components/workspace/index.ts`
- Modify: `App.tsx` and cross-importers

- [x] **Step 1–4:** same pattern as Task 4
- [x] **Commit:** `refactor(ui): move workspace components into components/workspace`

---

### Task 6: agents domain

**Files:**
- Move: agents cluster
- Create: `apps/desktop/src/components/agents/index.ts`
- Modify: `App.tsx` (`ModelPicker`), chat/settings importers of Agent*/Model*

- [x] **Step 1–4:** same pattern
- [x] **Commit:** `refactor(ui): move agent/model pickers into components/agents`

---

### Task 7: schedule domain

**Files:**
- Move: `CronPanel.tsx`, `CreateCronDialog.tsx`, `ScheduleEditor.tsx`
- Create: `apps/desktop/src/components/schedule/index.ts`
- Modify: `App.tsx` and any importers

- [x] **Step 1–4:** same pattern
- [x] **Commit:** `refactor(ui): move cron/schedule components into components/schedule`

---

### Task 8: settings domain + final sweep

**Files:**
- Move: settings cluster
- Create: `apps/desktop/src/components/settings/index.ts`
- Modify: `App.tsx` and remaining importers
- Verify: `components/` root contains only domain dirs (no leftover `.tsx` at root)

- [x] **Step 1: git mv settings cluster**
- [x] **Step 2: Fix imports + consumers**
- [x] **Step 3: Repo-wide search for stale paths**

```bash
cd frontend && rg "from ['\"].*components/(Chat|Msg|FileSpace|Workspace|Toast|NavIcons|Cron|Preferences|Agent|Model)" src
```

Expected: no matches to old flat paths (except comments if any).

- [x] **Step 4: Confirm root is dirs-only**

```bash
ls apps/desktop/src/components
# expect: agents chat filespace icons media schedule settings ui workspace (+ maybe no loose tsx)
```

- [x] **Step 5: `cd frontend && npm run build`**
- [x] **Step 6: Commit** `refactor(ui): move settings panels into components/settings`
- [x] **Step 7: Update spec status line to 已实现** in the design doc; commit if changed

---

## Spec coverage check

| Spec item | Task |
|-----------|------|
| Domain folders + file ownership | Tasks 1–8 File map |
| Optional barrels | Steps in each task |
| No root shims / full import update | Each consumer step + Task 8 sweep |
| Order icons→ui→chat→…→settings | Task order |
| No App split / no hooks-lib move | Global Constraints |
| Verify build per domain | Each task verify step |
