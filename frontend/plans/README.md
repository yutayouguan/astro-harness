# Animation Improvement Plans

Audit commit: `5b1b3166` | Audited: 2026-08-08

## Plans

| # | Title | Severity | Status | Deps |
|---|-------|----------|--------|------|
| 001 | Fix anim-switch exit ease-in | HIGH | DONE | — |
| 002 | Eliminate transition: all in loop.css | HIGH | DONE | — |
| 003 | Add dialog button :active feedback | HIGH | DONE | — |
| 004 | Add loop button :active feedback | MEDIUM | DONE | 002 |
| 005 | Add prefers-reduced-motion to 7 files | MEDIUM | DONE | — |
| 006 | Unify entry animation easing | MEDIUM | DONE | — |
| 007 | Fix mem-flow-pulse layout animation | MEDIUM | DONE (will-change) | — |
| 008 | Fix radio checkmark scale(0) | LOW | DONE | — |
| 009 | Replace width/height bar animations | LOW | DONE (2/3, insights skipped) | — |
| 010 | Reduce focus-flash duration | LOW | DONE | — |
| 011 | Tooltip skip delay on consecutive hover | LOW | DONE | — |
| 012 | Add dialog exit animation | LOW | DONE | — |

## Recommended execution order

**Phase 1 — Quick wins (CSS-only, no dependencies):**
1. 001 — ease-in fix (1 line, biggest feel impact)
2. 002 — transition: all (6 lines)
3. 003 — dialog :active (8 lines added)
4. 006 — easing unification (6 lines)
5. 010 — focus-flash duration (1 line)
6. 008 — radio scale(0) (3 lines)

**Phase 2 — Medium effort:**
7. 004 — loop :active (depends on 002)
8. 005 — reduced-motion coverage (7 files, ~50 lines)

**Phase 3 — Requires JS changes:**
9. 007 — mem-flow-pulse (CSS keyframe refactor or will-change)
10. 009 — progress bar scaleX/scaleY (CSS + React)
11. 011 — tooltip consecutive hover (CSS + JS controller)
12. 012 — dialog exit animation (CSS + React)
