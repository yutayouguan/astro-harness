# 002 — Eliminate transition: all in loop.css

- **Status**: TODO
- **Commit**: 5b1b3166
- **Severity**: HIGH
- **Category**: Performance
- **Estimated scope**: 1 file, 6 lines

## Problem

Six elements in `loop.css` use `transition: all`, which forces the browser to check every computed property for interpolation on each frame. These elements only change `background`, `color`, `border-color`, or `filter` on hover/active — `all` needlessly transitions layout properties like `padding`, `border-radius`, `width`, etc.

```css
/* src/styles/features/loop.css:80 — current */
.loop-view-btn { transition: all 0.15s; }

/* src/styles/features/loop.css:112 — current */
.loop-btn { transition: all 0.15s; }

/* src/styles/features/loop.css:143 — current */
.loop-icon-btn { transition: all 0.15s; }

/* src/styles/features/loop.css:841 — current */
.loop-palette-item { transition: all 0.12s; }

/* src/styles/features/loop.css:1141 — current */
.loop-rf-toolbar-btn { transition: all 0.12s; }

/* src/styles/features/loop.css:2164 — current */
.loop-ai-suggestion { transition: all 0.12s; }
```

## Target

```css
/* target — specify only the properties that actually change */
.loop-view-btn { transition: background 0.15s ease, color 0.15s ease, box-shadow 0.15s ease; }
.loop-btn { transition: background 0.15s ease, color 0.15s ease, filter 0.15s ease; }
.loop-icon-btn { transition: background 0.15s ease, color 0.15s ease; }
.loop-palette-item { transition: background 0.12s ease; }
.loop-rf-toolbar-btn { transition: background 0.12s ease, color 0.12s ease; }
.loop-ai-suggestion { transition: background 0.12s ease, border-color 0.12s ease, color 0.12s ease; }
```

## Repo conventions to follow

- The rest of the codebase specifies exact properties: e.g. `src/styles/features/filespace.css:105` uses `transition: background 0.14s ease, color 0.14s ease, box-shadow 0.14s ease;`.
- Use `ease` as the timing function (matches sibling hover transitions throughout the project).

## Steps

1. In `src/styles/features/loop.css`, line 80, replace `transition: all 0.15s;` with `transition: background 0.15s ease, color 0.15s ease, box-shadow 0.15s ease;`.
2. Line 112, replace `transition: all 0.15s;` with `transition: background 0.15s ease, color 0.15s ease, filter 0.15s ease;`.
3. Line 143, replace `transition: all 0.15s;` with `transition: background 0.15s ease, color 0.15s ease;`.
4. Line 841, replace `transition: all 0.12s;` with `transition: background 0.12s ease;`.
5. Line 1141, replace `transition: all 0.12s;` with `transition: background 0.12s ease, color 0.12s ease;`.
6. Line 2164, replace `transition: all 0.12s;` with `transition: background 0.12s ease, border-color 0.12s ease, color 0.12s ease;`.

## Boundaries

- Do NOT change any property other than the `transition` declaration on each line.
- Do NOT add new selectors or restructure the CSS.
- If a step doesn't match the code you find (drift since the commit stamp), STOP and report.

## Verification

- **Mechanical**: `grep -n "transition: all" src/styles/features/loop.css` — should return zero matches.
- **Feel check**: Open the Loop (智能流程) page. Hover over view-mode buttons, primary/secondary buttons, icon buttons, palette items, toolbar buttons, and AI suggestions. Each should smoothly transition background/color on hover, with no popping or missing transitions.
- **Done when**: Zero instances of `transition: all` in `loop.css`.
