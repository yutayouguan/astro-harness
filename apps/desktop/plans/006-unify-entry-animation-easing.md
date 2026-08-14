# 006 — Unify entry animation easing to project custom curve

- **Status**: TODO
- **Commit**: 5b1b3166
- **Severity**: MEDIUM
- **Category**: Easing cohesion
- **Estimated scope**: 4 files, 6 lines

## Problem

Six entry animations use default `ease` (`cubic-bezier(0.25, 0.1, 0.25, 1)`) instead of the project's custom entry curve `cubic-bezier(0.22, 1, 0.36, 1)`. The default `ease` has a slow-start phase that makes elements feel hesitant on appearance. Other animations in the same files already use the correct curve, creating inconsistency.

```css
/* src/styles/features/chat/core.css:59 — current */
.msg-row { animation: rise 0.3s ease both; }

/* src/styles/features/memory.css:578 — current */
.evo-card { animation: evo-card-in 0.22s ease both; }

/* src/styles/features/loop.css:928 — current */
.loop-picker { animation: loop-picker-in 0.15s ease; }

/* src/styles/features/cron.css:416 — current */
.cron-xxx { animation: rise 0.16s ease both; }

/* src/styles/features/cron.css:1231 — current */
.cron-dt-xxx { animation: cron-dt-pop-in 0.16s ease both; }

/* src/styles/features/skills/detail.css:1515 — current */
.skills-loading { animation: skills-loading-fade 0.22s ease; }
```

## Target

Replace `ease` with `cubic-bezier(0.22, 1, 0.36, 1)` in each:

```css
.msg-row { animation: rise 0.3s cubic-bezier(0.22, 1, 0.36, 1) both; }
.evo-card { animation: evo-card-in 0.22s cubic-bezier(0.22, 1, 0.36, 1) both; }
/* etc. */
```

## Repo conventions to follow

- Canonical usage: `src/styles/features/cron.css:237` — `animation: rise 0.18s cubic-bezier(0.22, 1, 0.36, 1) both;`
- The curve is also defined as `--anim-ease` in `transitions.css:6`, but since `animation` shorthand can't reference CSS variables for timing functions in all browsers, inline the curve value.

## Steps

1. `src/styles/features/chat/core.css:59` — change `ease` to `cubic-bezier(0.22, 1, 0.36, 1)`.
2. `src/styles/features/memory.css:578` — change `ease` to `cubic-bezier(0.22, 1, 0.36, 1)`.
3. `src/styles/features/loop.css:928` — change `ease` to `cubic-bezier(0.22, 1, 0.36, 1)`.
4. `src/styles/features/cron.css:416` — change `ease` to `cubic-bezier(0.22, 1, 0.36, 1)`.
5. `src/styles/features/cron.css:1231` — change `ease` to `cubic-bezier(0.22, 1, 0.36, 1)`.
6. `src/styles/features/skills/detail.css:1515` — change `ease` to `cubic-bezier(0.22, 1, 0.36, 1)`.

## Boundaries

- Do NOT change durations.
- Do NOT change keyframe definitions.
- Do NOT change animations that correctly use `ease-out`, `ease-in-out`, or the custom curve already.
- If a line doesn't match (drift), STOP and report.

## Verification

- **Mechanical**: `grep -rn "animation:.*ease[^-]" src/styles/features/ src/styles/components/` — the 6 changed lines should no longer appear. (Some default `ease` on hover transitions is fine — only check `animation:` lines.)
- **Feel check**: Send a message in chat — the `.msg-row` should snap in crisply. Open Evolution page — `.evo-card` entries should feel punchy. Open Cron — rows and date pickers should appear with the same snappy curve as the rest of the page.
- **Done when**: All entry `animation:` declarations use `cubic-bezier(0.22, 1, 0.36, 1)` or `ease-out`, not bare `ease`.
