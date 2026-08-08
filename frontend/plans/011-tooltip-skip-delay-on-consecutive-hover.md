# 011 — Skip tooltip delay on consecutive hover

- **Status**: TODO
- **Commit**: 5b1b3166
- **Severity**: LOW (missed opportunity)
- **Category**: Cohesion — tooltip UX
- **Estimated scope**: 1 CSS file + 1 JS/TS file

## Problem

The tooltip system has a 280ms delay (`--tip-delay: 0.28s`) and a 140ms animation on every tooltip. When hovering across adjacent sidebar buttons or toolbar icons, each tooltip independently waits 280ms then animates in, making the toolbar feel sluggish.

Best practice (Sonner/Radix pattern): once the first tooltip is open, subsequent tooltips in the same group should appear instantly (0ms delay, 0ms animation).

```css
/* src/styles/components/tooltip.css:5 — current */
[data-tip] {
  --tip-delay: 0.28s;
}

/* tooltip.css:53-56 — current */
.ui-tip {
  transition:
    opacity 0.14s ease,
    transform 0.14s ease;
}
```

## Target

```css
/* target — add instant mode */
.ui-tip[data-instant] {
  transition-duration: 0ms;
}
```

The JS tooltip controller should track a "group open" state: when any tooltip is visible, set `data-instant` on subsequent tooltips. Clear the state after a short idle period (~200ms after last tooltip closes).

## Repo conventions to follow

- The tooltip system is in `src/components/ui/` — look for the tooltip controller/hook.
- The `data-show="1"` attribute controls visibility already.

## Steps

1. In `src/styles/components/tooltip.css`, after line 191 (the last `data-show` rule), add:
   ```css
   .ui-tip[data-instant] {
     transition-duration: 0ms;
   }
   ```
2. In the tooltip JS controller (find via `grep -rn "ui-tip\|data-tip\|data-show" src/ --include="*.ts" --include="*.tsx"`):
   - Track a module-level `lastCloseTime: number`.
   - When showing a tooltip: if `Date.now() - lastCloseTime < 200`, set `data-instant` on the `.ui-tip` element.
   - When hiding a tooltip: record `lastCloseTime = Date.now()` and remove `data-instant`.
3. In the `@media (prefers-reduced-motion: reduce)` block, ensure `data-instant` doesn't override the `transition: none` already set.

## Boundaries

- Do NOT change the initial delay (280ms is correct for the first tooltip).
- Do NOT change tooltip positioning or arrow logic.

## Verification

- **Feel check**: Hover over the sidebar nav icons quickly in sequence. The first tooltip should delay normally; the second and subsequent should appear instantly with no animation. After pausing for ~250ms, the next tooltip should delay again.
- **Done when**: Rapid consecutive tooltip hovers feel instant instead of delayed.
