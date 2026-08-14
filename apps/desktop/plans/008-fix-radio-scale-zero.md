# 008 — Fix radio checkmark scale(0) entry

- **Status**: TODO
- **Commit**: 5b1b3166
- **Severity**: LOW
- **Category**: Physicality
- **Estimated scope**: 1 file, 1 line

## Problem

The model radio button checkmark pseudo-element animates from `scale(0)` (invisible singularity) to `scale(1)`. The easing function has nothing visible to ease *from*, making the entrance feel like it pops from nothing.

```css
/* src/styles/features/providers.css:1623 — current */
.providers-model-radio::before {
  transform: scale(0);
  transition: transform 0.12s ease-out;
}

/* providers.css:1638 */
.providers-model-radio:checked::before {
  transform: scale(1);
}
```

## Target

```css
/* target */
.providers-model-radio::before {
  transform: scale(0);
  opacity: 0;
  transition: transform 0.12s ease-out, opacity 0.08s ease-out;
}

.providers-model-radio:checked::before {
  transform: scale(1);
  opacity: 1;
}
```

Adding `opacity: 0` → `1` bridges the visibility gap. The element fades in while scaling, masking the unnatural `scale(0)` origin. Ideally `scale(0)` would be `scale(0.6)`, but for a checkmark icon using CSS mask, the mask shape at `scale(0.6)` may look odd — the opacity crossfade is a safer fix.

## Repo conventions to follow

- Exemplar: `src/styles/components/toast.css:220-228` — toast entry uses `scale(0.98)` + `opacity: 0` combined.

## Steps

1. In `src/styles/features/providers.css`, line 1623, add `opacity: 0;` after `transform: scale(0);`.
2. On line 1624, change `transition: transform 0.12s ease-out;` to `transition: transform 0.12s ease-out, opacity 0.08s ease-out;`.
3. On line 1638 (`.providers-model-radio:checked::before`), add `opacity: 1;` after `transform: scale(1);`.

## Boundaries

- Do NOT change the radio button's outer ring styles.
- Do NOT change the mask/icon shape.

## Verification

- **Mechanical**: CSS-only.
- **Feel check**: Go to Providers page, change the selected model. The checkmark should fade-and-scale in smoothly rather than popping from invisible. In DevTools at 25% speed, the checkmark should be visible throughout the transition, not just at the end.
- **Done when**: The radio checkmark transition includes opacity, smoothing the `scale(0)` → `scale(1)` appearance.
