# Token contract

The application imports token files directly from `styles/index.css` in this order:

1. `tokens.primitive`: theme-independent geometry, elevation, motion, and raw theme palettes.
2. `tokens.semantic`: light/dark meaning (`--color-*`, `--surface-panel-*`).
3. `tokens.component`: glass, menu, header, badge, and segmented-control recipes.
4. `overrides`: document/local tone mapping, unified color, glass intensity, and accessibility preferences.

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
