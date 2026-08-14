# 010 — Reduce focus-flash box-shadow duration

- **Status**: TODO
- **Commit**: 5b1b3166
- **Severity**: LOW
- **Category**: Duration
- **Estimated scope**: 1 file, 1 line

## Problem

The message focus-flash effect uses a 350ms `box-shadow` transition, which makes the highlight linger instead of snapping to attention.

```css
/* src/styles/features/chat/core.css:373 — current */
.msg-row.is-focus-flash .bubble {
  box-shadow:
    0 0 0 2px color-mix(in srgb, var(--tone-cyan, #06b6d4) 55%, transparent),
    0 8px 24px color-mix(in srgb, var(--tone-cyan, #06b6d4) 18%, transparent);
  transition: box-shadow 0.35s ease;
}
```

The rest of the codebase uses 0.12s–0.22s for comparable state transitions.

## Target

```css
/* target */
.msg-row.is-focus-flash .bubble {
  transition: box-shadow 0.22s ease;
}
```

220ms is fast enough to feel responsive while still being perceptible as a highlight flash.

## Repo conventions to follow

- Comparable: sidebar indicator transition is 220ms. Dialog entry is 180ms.

## Steps

1. In `src/styles/features/chat/core.css`, line 373, change `0.35s` to `0.22s`.

## Boundaries

- Do NOT change the box-shadow values (colors, spread).
- Do NOT change other transitions on `.bubble`.

## Verification

- **Mechanical**: CSS-only.
- **Feel check**: Click on a session in the sidebar that scrolls to a specific message. The message bubble should flash its cyan highlight ring snappily, drawing the eye without lingering.
- **Done when**: The focus-flash transition is 220ms.
