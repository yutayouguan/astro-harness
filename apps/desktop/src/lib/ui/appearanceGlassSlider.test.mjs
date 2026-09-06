import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [preferences, styles] = await Promise.all(
  [
    "../../components/settings/PreferencesPanel.tsx",
    "../../styles/features/preferences.css",
  ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
);

test("glass intensity uses one accessible five-stop range", () => {
  const control = preferences.slice(
    preferences.indexOf('className="appearance-glass-slider"'),
    preferences.indexOf("prefs-card--appearance-color"),
  );

  assert.match(control, /type="range"/);
  assert.match(control, /min=\{0\}/);
  assert.match(control, /max=\{glassOptions\.length - 1\}/);
  assert.match(control, /step=\{1\}/);
  assert.match(
    control,
    /aria-valuetext=\{glassOptions\[glassLevelIndex\]\?\.label\}/,
  );
  assert.match(control, /setGlassLevel\(next\.id\)/);
  assert.doesNotMatch(control, /role="radiogroup"/);
});

test("glass range exposes discrete ticks, progress, focus, and reduced motion", () => {
  assert.equal(
    preferences.match(/appearance-control-row appearance-control-row--split/g)
      ?.length,
    2,
  );
  assert.match(
    styles,
    /\.appearance-control-row--split\s*\{[\s\S]*?grid-template-columns:\s*minmax\(0, 0\.88fr\) minmax\(0, 1\.12fr\);/,
  );
  assert.match(
    styles,
    /\.appearance-control-row--split > :last-child\s*\{[\s\S]*?width:\s*100%;[\s\S]*?min-width:\s*0;[\s\S]*?max-width:\s*380px;[\s\S]*?box-sizing:\s*border-box;/,
  );
  assert.doesNotMatch(styles, /\.appearance-glass-slider\s*\{[^}]*cqi/);
  assert.match(
    styles,
    /\.appearance-glass-slider-ticks\s*\{[\s\S]*?grid-template-columns:\s*repeat\(5, 1fr\);/,
  );
  assert.match(
    styles,
    /\.appearance-glass-range::\-webkit-slider-runnable-track[\s\S]*?--glass-range-progress/,
  );
  assert.match(
    styles,
    /\.appearance-glass-range:focus-visible::\-webkit-slider-thumb/,
  );
  assert.match(
    styles,
    /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.appearance-glass-slider-labels span[\s\S]*?transition:\s*none;/,
  );
});
