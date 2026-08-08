# 004 — Add :active press feedback to loop page buttons

- **Status**: TODO
- **Commit**: 5b1b3166
- **Severity**: MEDIUM
- **Category**: Physicality & press feedback
- **Estimated scope**: 1 file, ~12 lines added

## Problem

The Loop (智能流程) page has multiple button classes with hover effects but zero `:active` press feedback: `.loop-btn`, `.loop-view-btn`, `.loop-icon-btn`, `.loop-rf-toolbar-btn`, `.loop-ai-send-btn`. Clicking these buttons — including the primary "运行" button — gives no physical response.

```css
/* src/styles/features/loop.css:112 — current (after Plan 002 fix) */
.loop-btn { transition: background 0.15s ease, color 0.15s ease, filter 0.15s ease; }
/* No :active anywhere for .loop-btn, .loop-view-btn, .loop-icon-btn, .loop-rf-toolbar-btn */
```

## Target

```css
.loop-btn:active:not(:disabled),
.loop-view-btn:active,
.loop-icon-btn:active {
  transform: scale(0.97);
}

.loop-rf-toolbar-btn:active {
  transform: scale(0.95);
}

.loop-ai-send-btn:active:not(:disabled) {
  transform: scale(0.96);
}
```

## Repo conventions to follow

- Exemplar: `src/styles/features/shell/header.css:75-76` — `.header-icon-btn:active { transform: scale(0.96); }`
- Larger buttons (`.loop-btn`) use 0.97; small icon buttons (`.loop-rf-toolbar-btn`) use 0.95; send button matches main `.send-btn` scale(0.96).
- Add `transform` to the transition list for each element.

## Steps

1. In `src/styles/features/loop.css`, line 80 (`.loop-view-btn` transition), append `, transform 0.12s ease` to the transition value.
2. Line 112 (`.loop-btn` transition), append `, transform 0.12s ease`.
3. Line 143 (`.loop-icon-btn` transition), append `, transform 0.12s ease`.
4. Line 1141 (`.loop-rf-toolbar-btn` transition), append `, transform 0.12s ease`.
5. After line 148 (`.loop-icon-btn:hover { ... }` block), add:
   ```css
   .loop-btn:active:not(:disabled),
   .loop-view-btn:active,
   .loop-icon-btn:active {
     transform: scale(0.97);
   }
   ```
6. After line 1154 (`.loop-rf-toolbar-btn--danger:hover { ... }`), add:
   ```css
   .loop-rf-toolbar-btn:active {
     transform: scale(0.95);
   }
   ```
7. Find `.loop-ai-send-btn` (around line 2305). Ensure `transform` is in its transition. After its hover rule, add:
   ```css
   .loop-ai-send-btn:active:not(:disabled) {
     transform: scale(0.96);
   }
   ```

## Boundaries

- Do NOT change hover behavior or existing visual styles.
- This plan depends on Plan 002 (transition: all → specific properties). Execute Plan 002 first.

## Verification

- **Mechanical**: CSS-only.
- **Feel check**: Open Loop page. Click and hold each button type — primary run button, view-mode toggles, icon buttons, node toolbar buttons, AI send button. Each should compress slightly on press.
- **Done when**: All five button classes respond to `:active` with visible scale feedback.
