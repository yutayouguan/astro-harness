import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const css = await readFile(new URL("../../styles/features/loop/responsive-overlays.css", import.meta.url), "utf8");
function rule(selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = css.match(new RegExp(`${escaped}\\s*\\{([^}]+)\\}`));
  assert.ok(match, `missing rule: ${selector}`);
  return match[1];
}

test("workflow empty templates use available width instead of a fixed centered column", () => {
  const templates = rule(".loop-empty-templates");
  assert.match(templates, /width:\s*100%/);
  assert.doesNotMatch(templates, /max-width/);
  assert.match(rule(".loop-empty-with-templates"), /padding:\s*12px 0 24px/);
});

test("empty-state grid adapts columns and permits narrow-panel reflow", () => {
  assert.match(rule(".loop-empty-templates .loop-template-grid"),
    /repeat\(auto-fit,\s*minmax\(min\(100%,\s*220px\),\s*1fr\)\)/);
  assert.match(rule(".loop-empty-templates .loop-template-card"), /min-width:\s*0/);
  assert.match(rule(".loop-empty-illust .astro-empty-hint"), /max-width:\s*52ch/);
});
