# 005 — Add prefers-reduced-motion to 7 uncovered files

- **Status**: TODO
- **Commit**: 5b1b3166
- **Severity**: MEDIUM
- **Category**: Accessibility
- **Estimated scope**: 7 files, ~50 lines added

## Problem

Seven CSS files define `@keyframes` animations but have zero `prefers-reduced-motion` handling. Users who enable reduced motion still see drawer slides, spinner rotations, and pulse animations.

| File | Keyframes | Type |
|------|-----------|------|
| `chat/drawers.css` | `agent-icon-drawer-in` | 220ms slide-in |
| `features/tools.css` | `tools-icon-spin`, `mcp-add-drawer-in` | infinite spin, 220ms slide-in |
| `features/providers.css` | `providers-dot-pulse`, `providers-icon-spin`, `providers-icon-pulse`, `providers-detail-in` | 3 infinite + 1 entry |
| `features/loop.css` | `loop-picker-in`, `loop-pulse`, `loop-spin` | 1 entry + 2 infinite |
| `skills/preview.css` | `skills-drawer-in` | 220ms slide-in |
| `skills/detail.css` | `skills-loading-fade` | 220ms fade |
| `skills/core.css` | `skills-icon-spin` | infinite spin |

## Target

Each file gets a `@media (prefers-reduced-motion: reduce)` block at the end that:
- **Removes** position/transform movement (slide-ins, spins)
- **Keeps** opacity transitions that aid comprehension (pulses can keep opacity-only)
- Follows the project pattern: set `animation: none` on the selector that applies the animation

## Repo conventions to follow

- Exemplar: `src/styles/components/toast.css:230-250` — groups all animated selectors in one `@media` block at file end.
- Pattern: `animation: none` on the element, not on the keyframe.
- Spinners: `animation: none` (they indicate loading state via other cues too).
- Drawer slide-ins: `animation: none` (content appears instantly).

## Steps

1. **`src/styles/features/chat/drawers.css`** — append at end:
   ```css
   @media (prefers-reduced-motion: reduce) {
     .agent-icon-drawer {
       animation: none;
     }
   }
   ```

2. **`src/styles/features/tools.css`** — append at end:
   ```css
   @media (prefers-reduced-motion: reduce) {
     .tools-checking-icon,
     .mcp-server-checking .mcp-server-icon {
       animation: none;
     }
     .mcp-add-drawer {
       animation: none;
     }
   }
   ```
   (Find the selectors that use `tools-icon-spin` and `mcp-add-drawer-in` by grepping in the file.)

3. **`src/styles/features/providers.css`** — append at end:
   ```css
   @media (prefers-reduced-motion: reduce) {
     .providers-status-dot.is-checking {
       animation: none;
     }
     .providers-icon-spin {
       animation: none;
     }
     .providers-icon-pulse {
       animation: none;
     }
     .providers-detail-row {
       animation: none;
     }
   }
   ```
   (Find each selector that references these keyframes by grepping `animation:` in the file.)

4. **`src/styles/features/loop.css`** — append at end:
   ```css
   @media (prefers-reduced-motion: reduce) {
     .loop-picker {
       animation: none;
     }
     .loop-node-running {
       animation: none;
     }
     .loop-ai-loading {
       animation: none;
     }
   }
   ```
   (Find each selector that references `loop-picker-in`, `loop-pulse`, `loop-spin`.)

5. **`src/styles/features/skills/preview.css`** — append at end:
   ```css
   @media (prefers-reduced-motion: reduce) {
     .skills-drawer {
       animation: none;
     }
   }
   ```

6. **`src/styles/features/skills/detail.css`** — append at end:
   ```css
   @media (prefers-reduced-motion: reduce) {
     .skills-loading {
       animation: none;
     }
   }
   ```

7. **`src/styles/features/skills/core.css`** — append at end:
   ```css
   @media (prefers-reduced-motion: reduce) {
     .skills-icon-loading {
       animation: none;
     }
   }
   ```

**Important**: The selector names above are approximate. Before editing, grep each file for `animation:.*<keyframe-name>` to find the exact selector that uses each keyframe. Use that selector in the `@media` block.

## Boundaries

- Do NOT change any keyframe definition.
- Do NOT change non-animation styles.
- Do NOT touch files that already have `prefers-reduced-motion` handling.

## Verification

- **Mechanical**: `grep -rn "prefers-reduced-motion" src/styles/features/chat/drawers.css src/styles/features/tools.css src/styles/features/providers.css src/styles/features/loop.css src/styles/features/skills/` — should return at least one match per file.
- **Feel check**: In DevTools Rendering panel, enable "Emulate CSS media feature prefers-reduced-motion: reduce". Open each affected page (Tools, Providers, Loop, Skills) and confirm: no spinning icons, no drawer slides, no pulse animations. Content should appear instantly.
- **Done when**: All 7 files have at least one `@media (prefers-reduced-motion: reduce)` block covering their animations.
