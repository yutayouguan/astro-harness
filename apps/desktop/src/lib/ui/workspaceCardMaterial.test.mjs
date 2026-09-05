import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const glass = await readFile(
  new URL("../../styles/tokens/component/glass.css", import.meta.url),
  "utf8",
);
const a11y = await readFile(
  new URL("../../styles/tokens/a11y.css", import.meta.url),
  "utf8",
);
const glassIntensity = await readFile(
  new URL("../../styles/tokens/glass-intensity.css", import.meta.url),
  "utf8",
);
const cron = await readFile(
  new URL("../../styles/features/cron/templates.css", import.meta.url),
  "utf8",
);
const loop = await readFile(
  new URL("../../styles/features/loop/panel.css", import.meta.url),
  "utf8",
);
const skills = await readFile(
  new URL("../../styles/features/skills/detail.css", import.meta.url),
  "utf8",
);
const tools = await readFile(
  new URL("../../styles/features/tools.css", import.meta.url),
  "utf8",
);
const welcome = await readFile(
  new URL("../../styles/features/chat/markdown.css", import.meta.url),
  "utf8",
);

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(
    new RegExp(`(?:^|\\n)${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\}`),
  )?.groups?.body;
}

test("workspace template cards share the canonical content material", () => {
  assert.match(glass, /--content-card-border:\s*var\(--glass-edge\);/);
  assert.match(glass, /--content-card-background:\s*var\(--glass-fill\);/);
  assert.match(
    glass,
    /--content-card-shadow:\s*var\(--shadow-card\),\s*var\(--glass-rim\);/,
  );
  assert.match(glass, /--content-card-backdrop:\s*var\(--backdrop-glass\);/);

  for (const [name, materialRule] of [
    ["scheduled task", rule(cron, ".cron-template-card")],
    ["workflow", rule(loop, ".loop-template-card")],
    ["skill", rule(skills, ".tool-card.skill-card")],
    ["MCP", rule(tools, ".mcp-server-card")],
    ["welcome", rule(welcome, ".chat-welcome-card")],
  ]) {
    assert.ok(materialRule, `missing ${name} card rule`);
    assert.match(
      materialRule,
      /var\(\s*--content-card-background/,
      `${name} cards must use the shared background`,
    );
    assert.match(
      materialRule,
      /var\(\s*--content-card-border/,
      `${name} cards must use the shared border`,
    );
    assert.match(
      materialRule,
      /var\(\s*--content-card-backdrop/,
      `${name} cards must use the shared backdrop`,
    );
  }
});

test("workspace content cards become solid for accessibility preferences", () => {
  assert.match(
    a11y,
    /@media \(prefers-reduced-transparency: reduce\)[\s\S]*?--content-card-background:\s*var\(--surface-panel-background\);[\s\S]*?--content-card-backdrop:\s*none;/,
  );
  assert.match(
    a11y,
    /@media \(prefers-contrast: more\)[\s\S]*?--content-card-border:\s*var\(--color-border\);[\s\S]*?--content-card-backdrop:\s*none;/,
  );
});

test("workspace content cards follow the selected liquid glass strength", () => {
  for (const [level, recipe] of [
    ["liquid", "liquid-glass"],
    ["liquid-soft", "liquid-glass-soft"],
  ]) {
    const escaped = level.replace("-", "\\-");
    const block = glassIntensity.match(
      new RegExp(
        `\\[data-glass="${escaped}"\\]\\s*\\{(?<body>[\\s\\S]*?)\\n\\}`,
      ),
    )?.groups?.body;
    assert.ok(block, `missing ${level} glass overrides`);
    assert.match(
      block,
      new RegExp(`--content-card-border:\\s*var\\(--${recipe}-edge\\);`),
    );
    assert.match(
      block,
      new RegExp(`--content-card-background:[\\s\\S]*?--${recipe}-sheen`),
    );
    assert.match(
      block,
      new RegExp(`--content-card-backdrop:\\s*var\\(--${recipe}-backdrop\\);`),
    );
  }
});

test("content cards keep tone in accents rather than their base fill", () => {
  assert.doesNotMatch(
    cron.match(/\.cron-template-card\s*\{(?<body>[\s\S]*?)\n\}/)?.groups
      ?.body ?? "",
    /--tone|--tone-soft/,
  );
  assert.doesNotMatch(
    skills.match(/\.tool-card\.skill-card\s*\{(?<body>[\s\S]*?)\n\}/)?.groups
      ?.body ?? "",
    /--skill-tone|--tone-soft/,
  );
  assert.doesNotMatch(
    welcome.match(/\.chat-welcome-card\s*\{(?<body>[\s\S]*?)\n\}/)?.groups
      ?.body ?? "",
    /color-mix\([^)]*--tone-soft/,
  );
});
