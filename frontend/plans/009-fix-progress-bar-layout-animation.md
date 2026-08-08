# 009 — Replace width/height animations with scaleX/scaleY on bars

- **Status**: TODO
- **Commit**: 5b1b3166
- **Severity**: LOW
- **Category**: Performance
- **Estimated scope**: 3 files, 3 lines changed + 3 lines added

## Problem

Three progress/chart bar elements animate `width` or `height` — layout-triggering properties that force relayout on every frame.

```css
/* src/styles/features/chat/activity.css:784 — current */
.a2ui-clarify-progress-fill {
  transition: width 320ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

/* src/styles/features/memory.css:957 — current */
.evo-budget-bar > span {
  transition: width 0.3s ease;
}

/* src/styles/features/insights.css:379 — current */
.insights-bar {
  transition: height 0.2s ease;
}
```

## Target

Use `transform: scaleX()` / `scaleY()` with appropriate `transform-origin` instead:

```css
/* activity.css target */
.a2ui-clarify-progress-fill {
  transform-origin: left center;
  transition: transform 320ms cubic-bezier(0.2, 0.8, 0.2, 1);
  /* width is set to 100%; scale controls the visible fill */
}

/* memory.css target */
.evo-budget-bar > span {
  transform-origin: left center;
  transition: transform 0.3s ease;
}

/* insights.css target */
.insights-bar {
  transform-origin: center bottom;
  transition: transform 0.2s ease;
}
```

**Caveat**: This change requires the JavaScript/React code that sets `width` / `height` to instead set `transform: scaleX(fraction)` / `scaleY(fraction)`. If the bars' widths are set via inline `style={{ width: "X%" }}`, those need to change to `style={{ transform: "scaleX(X/100)" }}` with the element having `width: 100%` as base.

## Repo conventions to follow

- Progress bars elsewhere in the app set width via inline styles. Check `.a2ui-clarify-progress-fill`, `.evo-budget-bar > span`, and `.insights-bar` in `.tsx` files for how the size is applied.

## Steps

1. **Before editing CSS**, grep for each selector in `.tsx` files to understand how the width/height is driven:
   - `grep -rn "clarify-progress-fill\|a2ui-clarify-progress" src/ --include="*.tsx"`
   - `grep -rn "evo-budget-bar" src/ --include="*.tsx"`
   - `grep -rn "insights-bar" src/ --include="*.tsx"`
2. If width/height is set via inline `style={{ width }}`:
   - Change the element's CSS `width` to `100%` (or `height` to the max value).
   - Add `transform-origin: left center` (or `center bottom` for height).
   - Change `transition: width ...` to `transition: transform ...`.
   - Update the React code to set `style={{ transform: \`scaleX(${fraction})\` }}` instead of `style={{ width: \`${pct}%\` }}`.
3. If width/height is set via CSS class toggling, adjust accordingly.

## Boundaries

- Do NOT change bar colors, border-radius, or visual appearance.
- If the React refactor is too complex for a particular bar, skip it and note in the plan — the performance impact is small for single bars.

## Verification

- **Mechanical**: `cd ui/frontend && npx tsc --noEmit` — must pass after React changes.
- **Feel check**: Each bar should fill smoothly without visual difference. In DevTools Performance panel, the bars should no longer trigger Layout events during their animation.
- **Done when**: At least the progress bars animate via `transform` instead of `width`/`height`.
