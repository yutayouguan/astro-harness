import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const root = new URL("../../", import.meta.url);

async function source(path) {
  return readFile(new URL(path, root), "utf8");
}

test("collapsed tool activity reads as a rounded status row", async () => {
  const [index, css] = await Promise.all([
    source("styles/index.css"),
    source("styles/features/chat/tool-activity-polish.css"),
  ]);

  assert.match(index, /activity-groups\.css[\s\S]*tool-activity-polish\.css/);
  assert.match(css, /> \.msg-activity \{/);
  assert.match(css, /border-radius: 15px/);
  assert.match(css, /min-height: 43px/);
  assert.doesNotMatch(css, /border-radius: 999px/);
});

test("expanded tool activity gains depth while running state avoids full-card glow", async () => {
  const css = await source("styles/features/chat/tool-activity-polish.css");

  assert.match(css, /> \.msg-activity\.is-open \{/);
  assert.match(css, /\.msg-activity\.is-open[\s\S]*\.msg-activity-collapse-inner/);
  assert.match(css, /> \.msg-activity\.is-running \{[\s\S]*animation: none/);
  assert.match(css, /prefers-reduced-motion: reduce/);
  assert.match(css, /prefers-reduced-transparency: reduce/);
});
