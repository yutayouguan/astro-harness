# 012 — Add dialog exit animation

- **Status**: TODO
- **Commit**: 5b1b3166
- **Severity**: LOW (missed opportunity)
- **Category**: Physicality — jarring change
- **Estimated scope**: 1 CSS file + 1 JS/TS file

## Problem

`app-dialog` and `about-dialog` have entry animations (`app-dialog-rise-in` at 180ms / 220ms), but closing them causes instant disappearance — no exit animation at all. The jarring vanish breaks spatial consistency: the dialog rises in gracefully but teleports away.

## Target

A quick fade + scale-down exit (120ms, faster than entry per asymmetric timing principle):

```css
@keyframes app-dialog-rise-out {
  from {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
  to {
    opacity: 0;
    transform: translateY(4px) scale(0.98);
  }
}

@keyframes app-dialog-fade-out {
  from { opacity: 1; }
  to { opacity: 0; }
}
```

## Repo conventions to follow

- Entry animation: `app-dialog-rise-in` at `0.18s cubic-bezier(0.22, 1, 0.36, 1)`.
- Exit should be faster: 120ms with `ease-out`.
- The dialog is rendered via React portal and unmounts on close — the exit animation must play *before* unmount.

## Steps

1. In `src/styles/components/dialog.css`, add the exit keyframes (above the `@media` block):
   ```css
   @keyframes app-dialog-rise-out {
     to {
       opacity: 0;
       transform: translateY(4px) scale(0.98);
     }
   }

   @keyframes app-dialog-fade-out {
     to { opacity: 0; }
   }

   .app-dialog-backdrop.is-closing {
     animation: app-dialog-fade-out 0.12s ease-out forwards;
   }

   .app-dialog-backdrop.is-closing .app-dialog,
   .app-dialog-backdrop.is-closing .about-dialog {
     animation: app-dialog-rise-out 0.12s ease-out forwards;
   }
   ```
2. Add to the `@media (prefers-reduced-motion: reduce)` block:
   ```css
   .app-dialog-backdrop.is-closing,
   .app-dialog-backdrop.is-closing .app-dialog,
   .app-dialog-backdrop.is-closing .about-dialog {
     animation: none;
   }
   ```
3. In the dialog React component (`grep -rn "app-dialog-backdrop" src/ --include="*.tsx"`):
   - Instead of immediately unmounting on close, add an `is-closing` class.
   - Listen for `animationend` on the backdrop element, then unmount.
   - With reduced-motion, skip the animation and unmount immediately.

## Boundaries

- Do NOT change dialog entry animation.
- Do NOT change dialog layout or visual styles.
- Keep exit at 120ms — do not exceed 150ms.

## Verification

- **Feel check**: Open any dialog, then close it. The backdrop should fade out while the dialog panel scales down and fades simultaneously. The close should feel quick (120ms) — no lingering.
  - Open → close rapidly — the exit should not block reopening.
  - Toggle `prefers-reduced-motion` — dialog should close instantly.
- **Done when**: Dialogs have a visible exit animation that completes before unmount.
