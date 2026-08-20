# Sidebar Icon Button Background Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make both sidebar icon buttons transparent by default and show transient background feedback only on hover and pointer press.

**Architecture:** Keep the existing shared `.sidebar-pin-btn` class so the titlebar toggle and sidebar label toggle remain visually consistent. Implement the behavior entirely with CSS pseudo-classes; React state and component markup remain unchanged.

**Tech Stack:** CSS, React 18, Vite 5

## Global Constraints

- Default state must not show a background or border.
- Hover must show the existing tone-aware shallow background.
- Pointer press must show a stronger tone-aware background.
- Releasing the pointer must not retain a selected background.
- Preserve existing dimensions, rounding, icon color, transitions, themes, and click behavior.

---

### Task 1: Make Sidebar Icon Button Feedback Transient

**Files:**
- Modify: `apps/desktop/src/styles/features/shell/shell.css:482-546`

**Interfaces:**
- Consumes: The existing `.sidebar-pin-btn` class used by both buttons in `apps/desktop/src/App.tsx`.
- Produces: Shared default, `:hover`, and `:active` visual states for `.sidebar-pin-btn`.

- [x] **Step 1: Record the failing visual baseline**

Run the app with:

```bash
cd frontend
npm run dev
```

Expected before the change: both sidebar icon buttons display a tinted background and border while idle, and pressing a button has no stronger background state.

- [x] **Step 2: Implement the three button states**

In `apps/desktop/src/styles/features/shell/shell.css`, change the base rule and add the press rule:

```css
.sidebar-pin-btn {
  margin-left: 0;
  width: 28px;
  height: 28px;
  border: 1px solid transparent;
  border-radius: 8px;
  display: grid;
  place-items: center;
  background: transparent;
  color: var(--tone, var(--ink-soft));
  cursor: pointer;
  flex-shrink: 0;
  transition: background 0.12s ease, color 0.12s ease, border-color 0.12s ease;
}

.sidebar-pin-btn:hover {
  background: color-mix(in srgb, var(--tone, var(--accent)) 22%, transparent);
  color: var(--tone, var(--ink));
}

.sidebar-pin-btn:active {
  background: color-mix(in srgb, var(--tone, var(--accent)) 32%, transparent);
}
```

- [x] **Step 3: Verify the frontend build**

Run:

```bash
cd frontend
npm run build
```

Expected: TypeScript and Vite complete successfully with exit code 0.

- [x] **Step 4: Verify both buttons visually**

In both light and dark themes, inspect the titlebar sidebar toggle and the sidebar label toggle.

Expected:

- Idle: no visible button background or border.
- Hover: a shallow tone-aware background appears.
- Pointer down: the background becomes stronger.
- Pointer up or pointer leave: no selected background remains.
- Both buttons still execute their existing actions.

- [x] **Step 5: Commit the implementation**

```bash
git add apps/desktop/src/styles/features/shell/shell.css
git commit -m "style(sidebar): make icon button feedback transient"
```
