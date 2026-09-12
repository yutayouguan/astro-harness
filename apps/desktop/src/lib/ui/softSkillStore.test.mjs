import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
const read = (path) => readFile(new URL(path, import.meta.url), "utf8");
const [source, css, iconCss] = await Promise.all([
  read("../../components/settings/SkillsPanel.tsx"),
  read("../../styles/materials/soft-skill-store.css"),
  read("../../styles/features/skills/cards.css"),
]);

test("online gallery alone receives cover layout, keeping other skill views compact", () => {
  assert.match(source, /skill-card skill-store-card/);
  assert.match(css, /\.skills-gallery\.is-gallery \.skill-store-card/);
  assert.doesNotMatch(css, /\.is-list|\.is-detail|\.skills-detail-item/);
  assert.match(css, /html\[data-material="soft"\]/);
  assert.match(css, /minmax\(min\(100%, 270px\), 1fr\)/);
});

test("card retains source, category, credential warning and install state", () => {
  const card = source.slice(
    source.indexOf("const renderStoreCard"),
    source.indexOf("const renderInstalledDetail"),
  );
  for (const value of [
    "skill.source",
    "skill.category",
    "skill.requires_api_key === true",
    'beginInstallSkill(skill, "direct")',
    'beginInstallSkill(skill, "agent")',
    "aria-busy={installingId === skill.id}",
    "disabled={installingId === skill.id}",
    "skills.alreadyInstalled",
  ])
    assert.ok(card.includes(value), value);
  assert.match(card, /viewStoreDetail\(skill\)/);
  assert.match(card, /copyStoreInstallCommand\(skill\)/);
});

test("failed store images are hidden and new image URLs remount without stale hidden state", () => {
  assert.match(source, /key=\{skill.icon_url\}/);
  assert.match(source, /event.currentTarget.hidden = true/);
  assert.match(
    iconCss,
    /\.skills-store-icon-image\[hidden\]\s*\{\s*display: none;/,
  );
  assert.match(source, /referrerPolicy="no-referrer"/);
});

test("store uses neutral primary filters and preserves credential badge styling", () => {
  assert.match(
    css,
    /\.skills-store-sort-tab\.is-active\s*\{[^}]*background: var\(--soft-ink\);[^}]*color: var\(--soft-base\);/,
  );
  assert.doesNotMatch(
    css,
    /requires-key|is-installed|\.primary\s*\{|!important/,
  );
  assert.match(css, /prefers-contrast: more/);
  assert.match(css, /min-block-size: calc\(3 \* 1\.55em \+ 1px\)/);
  assert.match(css, /flex: 1 0 auto/);
  assert.match(css, /min-height: max-content/);
  assert.match(css, /grid-auto-rows: max-content/);
});
