# 007 — Replace left/top animation with translate in mem-flow-pulse

- **Status**: TODO
- **Commit**: 5b1b3166
- **Severity**: MEDIUM
- **Category**: Performance
- **Estimated scope**: 1 file, ~20 lines

## Problem

`@keyframes mem-flow-pulse` animates `left` and `top` — layout-triggering properties — across 6 keyframe stops. Six variant elements (`.mem-view-flow-pulse--0` through `--5`) run this animation simultaneously with different durations, multiplying the layout cost on every frame.

```css
/* src/styles/features/memory.css:198-221 — current */
@keyframes mem-flow-pulse {
  0% {
    left: 2%;
    top: 34%;
    opacity: 0;
    transform: scale(0.6);
  }
  10% {
    opacity: 1;
  }
  48% {
    left: 48%;
    top: var(--mid-y, 72%);
    transform: scale(1);
  }
  90% {
    opacity: 0.85;
  }
  100% {
    left: 96%;
    top: 36%;
    opacity: 0;
    transform: scale(0.55);
  }
}
```

## Target

Replace `left`/`top` with `translate()` inside `transform`, and set the element's base position via `left`/`top` as a static starting point:

```css
@keyframes mem-flow-pulse {
  0% {
    opacity: 0;
    transform: translate(0, 0) scale(0.6);
  }
  10% {
    opacity: 1;
  }
  48% {
    transform: translate(calc(48% - 2%), calc(var(--mid-y, 72%) - 34%)) scale(1);
  }
  90% {
    opacity: 0.85;
  }
  100% {
    opacity: 0;
    transform: translate(calc(96% - 2%), calc(36% - 34%)) scale(0.55);
  }
}
```

Note: The `translate()` percentages in `transform` are relative to the element's own size, not the parent. Since these are small decorative dots, the parent-relative positioning needs a different approach. The cleanest solution is to use `position: absolute` with fixed `left: 0; top: 0` on the element, and move entirely via `transform: translate(Xvw, Yvh)` or pixel values calculated from the container.

**Alternative (simpler, recommended)**: Since these are purely decorative particles with `position: absolute`, pin them at `left: 0; top: 0` and use `transform: translate(Xpx, Ypx) scale(...)` where X/Y are computed from the container's known dimensions. If the container size is unknown at CSS time, use `will-change: transform` on the elements to hint the browser to promote them to their own compositing layer, reducing the layout cost even while animating `left`/`top`.

## Repo conventions to follow

- Other animations in the file use `transform` correctly (e.g. `mem-flow-ember` at line 186).
- If full refactor is too complex, the fallback is adding `will-change: transform, left, top` on the pulse elements to promote them to GPU layers.

## Steps

1. Find the selectors that apply `mem-flow-pulse` animation (grep `animation:.*mem-flow-pulse` in `memory.css`).
2. Add `will-change: transform` to each of those selectors (minimum viable fix).
3. If the container has known dimensions, refactor the keyframe to use `transform: translate(...)` instead of `left`/`top`. Replace the `left`/`top` values with equivalent `translate()` values.
4. Test that the particle animation visually matches the original trajectory.

## Boundaries

- Do NOT change the visual appearance of the particle animation.
- Do NOT change `mem-flow-ember` or other unrelated keyframes.
- If the container dimensions are dynamic/unknown, use the `will-change` approach rather than guessing pixel values.

## Verification

- **Mechanical**: CSS-only.
- **Feel check**: Open the Memory page. The decorative flow particles should follow the same path as before. In DevTools Performance panel, record a 3-second trace — layout events during the animation should be reduced compared to before.
- **Done when**: Either `left`/`top` are removed from keyframes and replaced with `translate()`, or `will-change` is added to the pulse elements.
