import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { readChatCss } from "./chatCssSource.mjs";

const glass = await readFile(
  new URL("../../styles/tokens/component/glass.css", import.meta.url),
  "utf8",
);
const a11y = await readFile(
  new URL("../../styles/tokens/a11y.css", import.meta.url),
  "utf8",
);
const immersiveLight = await readFile(
  new URL("../../styles/tokens/immersive-light.css", import.meta.url),
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
const welcome = await readChatCss();

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

test("workspace content cards follow continuous immersive light intensity", () => {
  const block = rule(immersiveLight, "[data-glass-intensity]");
  assert.ok(block, "missing continuous glass intensity overrides");
  assert.match(
    block,
    /--glass-blur-scale:\s*calc\(1\.4 \* var\(--glass-intensity, 0\.64\)\);/,
  );
  assert.match(
    block,
    /--content-card-border:\s*var\(--immersive-reference-border\);/,
  );
  assert.match(
    block,
    /--content-card-background:\s*var\(--immersive-reference-background\);/,
  );
  assert.match(
    block,
    /--content-card-shadow:\s*var\(--immersive-reference-shadow\);/,
  );
  assert.match(
    block,
    /--content-card-backdrop:\s*var\(--immersive-reference-backdrop\);/,
  );
  assert.doesNotMatch(immersiveLight, /data-glass="/);
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
