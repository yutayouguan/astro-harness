import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const preferencesCss = await readFile(
  new URL("../../styles/features/preferences.css", import.meta.url),
  "utf8",
);
const preferencesPanel = await readFile(
  new URL("../../components/settings/PreferencesPanel.tsx", import.meta.url),
  "utf8",
);

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(
    new RegExp(`(?:^|\\n)${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\}`),
  )?.groups?.body;
}

test("morphicon controls use a compact track and a distinct selection pill", () => {
  const card = rule(preferencesCss, ".morphicon-settings-card");
  const subtitle = rule(
    preferencesCss,
    ".morphicon-settings-card .prefs-card-sub",
  );
  const row = rule(preferencesCss, ".morphicon-setting-row");
  const track = rule(preferencesCss, ".morphicon-segmented");
  const active = rule(preferencesCss, ".morphicon-segmented button.is-active");
  const indicator = rule(preferencesCss, ".morphicon-selection-indicator");
  const darkIndicator = rule(
    preferencesCss,
    'html[data-theme="dark"] .morphicon-selection-indicator',
  );

  assert.ok(card, "missing Morphicon card treatment");
  assert.match(card, /display:\s*grid/);
  assert.doesNotMatch(
    card,
    /(?:background|border-color|box-shadow):/,
    "Morphicon card must inherit the shared settings panel material",
  );
  assert.match(
    preferencesCss,
    /\.prefs-card,\s*\.prefs-section\s*\{[\s\S]*?background:\s*var\(--settings-panel-background/,
  );
  assert.ok(subtitle, "missing Morphicon subtitle contrast treatment");
  assert.match(subtitle, /color:\s*var\(--ink-soft\)/);
  assert.ok(row, "missing Morphicon compact row layout");
  assert.match(
    row,
    /grid-template-columns:\s*minmax\(180px, 1fr\) minmax\(260px, 360px\)/,
  );
  assert.match(row, /min-height:\s*54px/);
  assert.ok(track, "missing Morphicon segmented track");
  assert.match(track, /gap:\s*2px/);
  assert.match(track, /padding:\s*3px/);
  assert.match(track, /var\(--ink\) 5%/);
  assert.ok(active, "missing Morphicon selected-state styles");
  assert.match(active, /font-weight:\s*700/);
  assert.ok(indicator, "missing Morphicon selection indicator");
  assert.match(indicator, /inset:\s*0/);
  assert.match(indicator, /border-radius:\s*8px/);
  assert.match(indicator, /var\(--tone\) 16%/);
  assert.match(indicator, /var\(--tone\) 30%/);
  assert.ok(darkIndicator, "missing dark-theme Morphicon selection pill");
  assert.match(darkIndicator, /var\(--tone\) 22%/);
  assert.match(darkIndicator, /var\(--tone\) 34%/);
});

test("morphicon selection moves with a reduced-motion-safe spring", () => {
  assert.match(preferencesPanel, /layoutId="morphicon-spring-selection"/);
  assert.match(preferencesPanel, /layoutId="morphicon-stroke-selection"/);
  assert.match(preferencesPanel, /useReducedMotion\(\)/);
  assert.match(
    preferencesPanel,
    /\{ type: "spring", bounce: 0, duration: 0\.32 \}/,
  );
  assert.match(preferencesPanel, /reduceMotion\s*\?\s*\{ duration: 0 \}/);
});
