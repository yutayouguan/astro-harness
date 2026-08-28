import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const panel = await readFile(
  new URL("../../components/settings/SkillsPanel.tsx", import.meta.url),
  "utf8",
);
const cards = await readFile(
  new URL("../../styles/features/skills/cards.css", import.meta.url),
  "utf8",
);
const detail = await readFile(
  new URL("../../styles/features/skills/detail.css", import.meta.url),
  "utf8",
);

test("Skill cards keep per-item color on the icon rather than the whole card", () => {
  assert.equal(panel.match(/data-skill-tone=\{skillTone\(skill\.id\)\}/g)?.length, 4);
  assert.match(cards, /\.tool-card\.skill-card \.tool-icon-glyph\s*\{[\s\S]*?var\(--skill-tone/);
  assert.match(detail, /\.skill-card-title\s*\{[\s\S]*?color:\s*var\(--ink\);/);
});

test("Skill descriptions use the card plane without a nested glass surface", () => {
  const descriptionRule = detail.match(
    /\.skill-card-desc\s*\{(?<body>[\s\S]*?)\n\}/,
  )?.groups?.body;

  assert.ok(descriptionRule, "missing Skill description rule");
  assert.match(descriptionRule, /background:\s*transparent;/);
  assert.match(descriptionRule, /border:\s*0;/);
  assert.match(descriptionRule, /box-shadow:\s*none;/);
  assert.doesNotMatch(descriptionRule, /backdrop-filter:\s*blur/);
});

test("Skill cards expose one text primary action and grouped secondary icons", () => {
  assert.match(panel, /className="skills-action-btn is-icon skill-card-update"/);
  assert.match(panel, /aria-label=\{t\("skills\.update"\)\}/);
  assert.match(detail, /\.skill-card-actions\s*\{[\s\S]*?border-top:/);
  assert.match(
    detail,
    /\.skill-card-actions \.skills-action-btn:not\(\.primary, \.is-installed\)/,
  );
});
