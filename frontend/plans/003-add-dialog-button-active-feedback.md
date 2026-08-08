# 003 — Add :active press feedback to dialog buttons

- **Status**: TODO
- **Commit**: 5b1b3166
- **Severity**: HIGH
- **Category**: Physicality & press feedback
- **Estimated scope**: 1 file, ~8 lines added

## Problem

`.app-dialog-btn` (confirm, cancel, danger-confirm) and `.about-dialog-ok` have hover transforms (`translateY(-0.5px)`) and a `transform` transition, but zero `:active` press feedback. Clicking a destructive "确认删除" button gives no tactile response — the button feels dead under the finger.

The project exemplar for press feedback is `.header-icon-btn:active { transform: scale(0.96); }` in `shell/header.css:75-76`.

```css
/* src/styles/components/dialog.css:195-200 — current */
.app-dialog-btn {
  transition:
    background 0.12s ease,
    border-color 0.12s ease,
    color 0.12s ease,
    box-shadow 0.12s ease,
    transform 0.12s ease;
}

/* dialog.css:223-228 — hover lifts, but no active pushes */
.app-dialog-btn.is-confirm:hover:not(:disabled) {
  transform: translateY(-0.5px);
}

/* dialog.css:487-496 — about-dialog-ok, same pattern */
.about-dialog-ok:hover {
  transform: translateY(-0.5px);
}
```

## Target

```css
/* target: after hover rules, add active pushdown */
.app-dialog-btn:active:not(:disabled) {
  transform: scale(0.97);
}

.about-dialog-ok:active {
  transform: scale(0.97);
}
```

## Repo conventions to follow

- Exemplar: `src/styles/features/shell/header.css:75-76` — `.header-icon-btn:active { transform: scale(0.96); }`
- Scale range: 0.95–0.98 (per Emil Kowalski). Use 0.97 for larger buttons.
- `transform` is already in the transition list, so the scale animates automatically.

## Steps

1. In `src/styles/components/dialog.css`, after line 248 (`.app-dialog-btn.is-confirm:disabled { ... }`), add:
   ```css
   .app-dialog-btn:active:not(:disabled) {
     transform: scale(0.97);
   }
   ```
2. After line 497 (`.about-dialog-ok:hover { ... }`), add:
   ```css
   .about-dialog-ok:active {
     transform: scale(0.97);
   }
   ```
3. In the existing `@media (prefers-reduced-motion: reduce)` block (line 270), add:
   ```css
   .app-dialog-btn:active:not(:disabled),
   .about-dialog-ok:active {
     transform: none;
   }
   ```

## Boundaries

- Do NOT change hover behavior or existing transitions.
- Do NOT add `:active` to other elements (loop buttons get their own plan).

## Verification

- **Mechanical**: CSS-only — no build step affected.
- **Feel check**: Open any dialog (e.g. delete an agent). Click and hold the "确认" or "取消" button — it should scale down to 0.97 on press and spring back on release. The press → release → hover cycle should feel smooth with no flicker.
  - Test the About dialog (关于 Astro) OK button the same way.
  - Toggle `prefers-reduced-motion` — the scale should be suppressed.
- **Done when**: `.app-dialog-btn:active` and `.about-dialog-ok:active` both produce visible press-down feedback.
