# 001 — Fix anim-switch exit ease-in

- **Status**: TODO
- **Commit**: 5b1b3166
- **Severity**: HIGH
- **Category**: Easing & duration
- **Estimated scope**: 1 file, 1 line

## Problem

Every panel/tab switch exit animation uses `ease-in`, which starts slow and accelerates — the opposite of what exit animations need. The user sees a sluggish departure on every single tab switch across the entire app.

```css
/* src/styles/foundation/transitions.css:48 — current */
.anim-switch.is-exit {
  animation: anim-switch-exit var(--anim-exit-ms) ease-in both;
  pointer-events: none;
}
```

The enter animation on line 44 correctly uses `var(--anim-ease)` (`cubic-bezier(0.22, 1, 0.36, 1)`). The asymmetry breaks the motion pair.

## Target

```css
/* target */
.anim-switch.is-exit {
  animation: anim-switch-exit var(--anim-exit-ms) ease-out both;
  pointer-events: none;
}
```

`ease-out` (starts fast, decelerates) makes the element leave quickly and fade gently — correct physics for an exit.

## Repo conventions to follow

- The enter animation on line 44 uses the project's custom curve `var(--anim-ease)` defined as `cubic-bezier(0.22, 1, 0.36, 1)` on line 6.
- For exit, built-in `ease-out` is sufficient and conventional — no need for a custom exit curve.

## Steps

1. In `src/styles/foundation/transitions.css`, line 48, change `ease-in` to `ease-out`:
   ```css
   animation: anim-switch-exit var(--anim-exit-ms) ease-out both;
   ```

## Boundaries

- Do NOT change the enter animation (line 44).
- Do NOT change `--anim-exit-ms` duration (140ms).
- Do NOT change the keyframe definition (`anim-switch-exit`).

## Verification

- **Mechanical**: `cd ui/frontend && npx tsc --noEmit` — should pass (CSS-only change, no TS impact).
- **Feel check**: Switch between any two tabs (e.g. Chat → Memory → Tools). The outgoing panel should now exit quickly and fade out gently, instead of hesitating before accelerating away.
  - In DevTools Animations panel, set playback to 25% and confirm the exit opacity drops sharply at the start, not at the end.
  - Toggle `prefers-reduced-motion` in Rendering panel — the existing `@media` block on line 100 already handles this (`animation: none`), so no regression.
- **Done when**: `ease-in` no longer appears in `transitions.css`.
