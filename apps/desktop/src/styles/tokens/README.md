# Token contract

The application imports token files directly from `styles/index.css` in this order:

1. `tokens.primitive`: theme-independent geometry, elevation, motion, and raw theme palettes.
2. `tokens.semantic`: light/dark meaning (`--color-*`, `--surface-panel-*`).
3. `tokens.component`: fallback glass, menu, header, badge, and segmented-control recipes.
4. `overrides`: document/local tone mapping, unified color, glass intensity, and accessibility preferences.

## Glass intensity

`component/glass.css` owns the fallback frosted recipe. `immersive-light.css` is the
single active recipe and maps the continuous strength onto every global surface family:
low-chroma environment color from wallpaper or Tab tone, stable translucent fill, soft
edge light, and restrained depth. Surfaces keep
consuming `--glass-fill` / `--glass-rim` / `--glass-edge` and need no per-file changes.
The persisted user value is an integer from `0` to `100`; `64` is the balanced default.

`--glass-blur-scale` and `--glass-saturate` are global knobs. Hardcoded
`backdrop-filter: blur(16px) saturate(1.25)` across feature styles was rewired to
`blur(calc(16px * var(--glass-blur-scale, 1))) saturate(var(--glass-saturate, 1.25))`
by `scripts/glass-knobs-codemod.mjs`, which keeps each component's own blur ratio while
letting one variable dim or intensify every glass surface at once. New glass styles should
either consume `--backdrop-glass` or follow that same knob form; re-running the codemod is
idempotent and will convert anything that regressed.

`data-glass-intensity` is not restricted to `html` — custom properties inherit, so
setting it together with a local `--glass-intensity` on a container enables a local
preview. `Design/Immersive Light Glass` in Storybook renders the 0, 64, and 100 calibration points
side by side.

## Content card material

`component/glass.css` maps the active glass recipe onto the canonical
`--content-card-*` contract: radius, border, background, shadow, and backdrop. Primary
cards in Settings, Cron templates, workflow templates, Skills/MCP, and ChatWelcome use
this contract so feature colors remain accents rather than competing base materials.
Small controls and nested rows intentionally keep lighter component-specific surfaces;
do not apply the content-card recipe recursively. Accessibility overrides replace the
background with the semantic panel surface and disable backdrop blur.
Content cards follow the selected continuous immersive-light strength. Accessibility
preferences still replace the recipe with a solid semantic surface.

`foundation/themes.css` and `foundation/tones.css` are deprecated barrels for legacy external entry points. The application does not import them, preventing duplicate token injection.

## Compatibility policy

Canonical tokens own values. Existing names such as `--bg0`, `--ink`, `--glass-panel`, and `--accent` alias the canonical contract in the same theme scope. Loop compatibility names map as follows:

- `--ink-secondary` → `--color-text-secondary`
- `--ink-tertiary` → `--color-text-tertiary`
- `--glass-hover` → `--surface-panel-hover`

Tone-dependent recipes that contain `var(--tone)` stay on the document theme scope or on the consuming segmented-control elements. Do not hoist those recipes to `:root`; local panel tones and portal tone forwarding must keep working.

## Removing compatibility aliases

Delete a legacy alias or the deprecated barrels only after:

1. repository search shows no runtime consumer or external stylesheet import;
2. light/dark, local-tone, unified/dynamic, portal, reduced-transparency, and increased-contrast stories are covered;
3. `lint:css`, `build`, `build-storybook`, and `test:visual` pass without snapshot updates;
4. one released migration window has elapsed for external theme consumers.
