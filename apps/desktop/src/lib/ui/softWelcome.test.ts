import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import {
  SOFT_WELCOME_CATEGORIES,
  SOFT_WELCOME_GROUPS,
  softWelcomeSections,
} from "./softWelcome.ts";

const cards = (
  Object.keys(SOFT_WELCOME_GROUPS) as (keyof typeof SOFT_WELCOME_GROUPS)[]
).map((id) => ({ id }));
test("all existing actions appear exactly once with two featured content cards", () => {
  const section = softWelcomeSections(cards, "all");
  assert.deepEqual(
    section.featured.map((card) => card.id),
    ["data", "image"],
  );
  const ids = [...section.featured, ...section.cards].map((card) => card.id);
  assert.equal(ids.length, 12);
  assert.equal(new Set(ids).size, 12);
});
test("category filters are lossless and never duplicate featured actions", () => {
  const groups = SOFT_WELCOME_CATEGORIES.filter(
    (category) => category !== "all",
  );
  const ids = groups.flatMap((category) => {
    const section = softWelcomeSections(cards, category);
    assert.equal(section.featured.length, 0);
    assert.ok(
      section.cards.every((card) => SOFT_WELCOME_GROUPS[card.id] === category),
    );
    return section.cards.map((card) => card.id);
  });
  assert.equal(ids.length, 12);
  assert.equal(new Set(ids).size, 12);
});
test("soft cards keep localized prompt slots, never send requests or animate a carousel", async () => {
  const source = await readFile(
    new URL("../../components/chat/SoftWelcome.tsx", import.meta.url),
    "utf8",
  );
  const root = await readFile(
    new URL("../../components/chat/ChatWelcome.tsx", import.meta.url),
    "utf8",
  );
  const css = await readFile(
    new URL("../../styles/materials/soft-welcome.css", import.meta.url),
    "utf8",
  );
  assert.match(source, /onPickCard\(prompt, promptTemplateHints\(prompt\)\)/);
  assert.match(root, /material === "soft"/);
  assert.match(root, /<GlassWelcome/);
  assert.doesNotMatch(
    source,
    /invoke\(|fetch\(|setInterval|ParticleField|Marquee/,
  );
  assert.match(css, /overflow: auto/);
  assert.match(css, /@container/);
  assert.match(css, /prefers-reduced-motion/);
  assert.match(css, /prefers-contrast/);
});

test("welcome cards use compact horizontal previews without shrinking body text", async () => {
  const css = await readFile(
    new URL("../../styles/materials/soft-welcome.css", import.meta.url),
    "utf8",
  );
  assert.match(css, /grid-template-columns: 44px minmax\(0, 1fr\)/);
  assert.match(css, /min-height: 84px/);
  assert.match(css, /grid-template-columns: 120px minmax\(0, 1fr\)/);
  assert.match(css, /min-height: 104px/);
  assert.match(css, /\.soft-welcome-card-copy strong\s*\{\s*font-size: 14px/);
  assert.match(css, /\.soft-welcome-card-copy > span\s*\{[^}]*font-size: 12px/);
  assert.doesNotMatch(css, /height: 132px|height: 70px|height: 110px/);
});

test("cards and category pills share the Start conversation material in every state", async () => {
  const css = await readFile(
    new URL("../../styles/materials/soft-welcome.css", import.meta.url),
    "utf8",
  );
  assert.match(
    css,
    /\.soft-welcome-start,\s*\.soft-welcome-card,\s*\.soft-welcome-filters button\s*\{\s*border: 1px solid var\(--soft-edge\);\s*background: var\(--soft-surface\);\s*box-shadow: var\(--soft-control-shadow\);/,
  );
  assert.doesNotMatch(css, /linear-gradient|background: var\(--soft-ink\)/);
  assert.match(css, /\[aria-pressed="true"\]::after/);
  assert.match(css, /background: currentColor/);
  assert.match(
    css,
    /prefers-contrast: more[\s\S]*\.soft-welcome-start,[\s\S]*box-shadow: none/,
  );
});
