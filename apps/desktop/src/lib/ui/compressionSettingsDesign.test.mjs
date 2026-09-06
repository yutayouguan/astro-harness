import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [component, css] = await Promise.all([
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
]);

test("automatic compression uses a page title and direct-manipulation ranges", () => {
  assert.match(component, /className="prefs-context-page-head"/);
  assert.match(component, /className="prefs-context-eyebrow"/);
  assert.match(component, /function CompressionRange/);
  assert.match(component, /type="range"/);
  assert.match(component, /onPointerUp=/);
  assert.match(component, /className="prefs-context-range-value"/);
  assert.match(component, /className="prefs-context-stage-accent"/);
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
