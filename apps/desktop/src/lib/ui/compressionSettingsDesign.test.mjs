import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [component, css, componentCss] = await Promise.all([
  readFile(
    new URL(
      "../../components/settings/CompressionSettingsCard.tsx",
      import.meta.url,
    ),
    "utf8",
  ),
  readFile(
    new URL("../../styles/features/preferences.css", import.meta.url),
    "utf8",
  ),
  readFile(
    new URL("../../styles/features/compression-settings.css", import.meta.url),
    "utf8",
  ),
]);

test("automatic compression keeps its compact card title and direct-manipulation ranges", () => {
  assert.doesNotMatch(component, /className="prefs-context-page-head"/);
  assert.doesNotMatch(component, /className="prefs-context-eyebrow"/);
  assert.match(component, /className="prefs-card-head"/);
  assert.match(component, /function CompressionRange/);
  assert.match(component, /type="range"/);
  assert.match(component, /onPointerUp=/);
  assert.match(component, /className="prefs-context-range-value"/);
  assert.match(component, /className="prefs-context-stage-accent"/);
});

test("automatic compression separates overview, stages, and advanced settings into cards", () => {
  for (const section of ["overview", "stages", "advanced"]) {
    assert.match(
      component,
      new RegExp(`prefs-context-card prefs-context-card--${section}`),
    );
  }
  assert.match(
    component,
    /<section className="prefs-card prefs-context-card prefs-context-card--advanced">[\s\S]*?<details className="prefs-context-advanced">/,
  );
  assert.match(
    componentCss,
    /\.prefs-page\.is-embedded \.prefs-context-card--advanced\s*\{[\s\S]*?padding:\s*0;/,
  );
});

test("compression ranges expose filled tracks, focus feedback, and responsive rows", () => {
  assert.match(
    css,
    /\.prefs-context-range-input::\-webkit-slider-runnable-track/,
  );
  assert.match(css, /var\(--range-progress\)/);
  assert.match(
    css,
    /\.prefs-context-range-input:focus-visible::\-webkit-slider-thumb/,
  );
  assert.match(
    css,
    /\.prefs-context-field--range\s*\{[\s\S]*?grid-template-columns:\s*minmax\(210px, 1fr\) minmax\(210px, 0\.82fr\);/,
  );
});
