import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [panel, styles] = await Promise.all([
  readFile(new URL("../../components/settings/SkillsPanel.tsx", import.meta.url), "utf8"),
  readFile(new URL("../../styles/features/skills/detail.css", import.meta.url), "utf8"),
]);

test("Skill updates surface exposes status, progress, and counted filters", () => {
  assert.match(panel, /className="skills-update-overview"/);
  assert.match(panel, /skills\.trackedBySkillHub/);
  assert.match(panel, /skills\.lastCheckedAt/);
  assert.match(panel, /skills-update-filter-count/);
  assert.match(panel, /aria-busy=\{checkingUpdates\}/);
  assert.match(panel, /updateChecksByFolder/);
  assert.match(panel, /className="skill-update-version-flow"/);
  assert.match(panel, /className="skill-update-version-arrow" aria-hidden>→/);
  assert.match(panel, /installedVersion \?\? "—"/);
  assert.match(styles, /\.skill-update-version-flow\s*\{/);
});

test("Skill updates surface adapts for narrow and accessibility contexts", () => {
  assert.match(styles, /@container skills-pane \(max-width: 680px\)[\s\S]*?skills-update-overview/);
  assert.match(styles, /@media \(prefers-contrast: more\)[\s\S]*?skills-update-overview/);
  assert.match(styles, /@media \(prefers-reduced-transparency: reduce\)[\s\S]*?skills-update-overview/);
});
