import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const root = new URL("../../", import.meta.url);

async function source(path) {
  return readFile(new URL(path, root), "utf8");
}

test("assistant reasoning and tool calls render as flat aligned rows", async () => {
  const [index, css] = await Promise.all([
    source("styles/index.css"),
    source("styles/features/chat/activity-flat-rows.css"),
  ]);

  assert.match(index, /activity-surfaces\.css[\s\S]*activity-flat-rows\.css/);
  assert.match(css, /\.msg-reasoning,[\s\S]*> \.msg-activity[\s\S]*border: 0;/);
  assert.match(
    css,
    /border-bottom: 0\.5px solid var\(--activity-flat-divider\)/,
  );
  assert.match(
    css,
    /border-radius: 0;[\s\S]*background: transparent;[\s\S]*box-shadow: none;/,
  );
  assert.match(
    css,
    /\.msg-reasoning-toggle,[\s\S]*\.msg-activity-toggle,[\s\S]*align-items: center;/,
  );
  assert.match(
    css,
    /\.msg-reasoning-chevron,[\s\S]*\.msg-activity-chevron[\s\S]*height: 18px;/,
  );
  assert.match(
    css,
    /\.msg-reasoning-toggle:focus-visible,[\s\S]*outline: 0;[\s\S]*box-shadow: inset/,
  );
  assert.match(
    css,
    /:has\([\s\S]*\.msg-reasoning, \.msg-activity[\s\S]*padding-top: 5px;/,
  );
  assert.match(
    css,
    /@media \(max-width: 480px\)[\s\S]*flex-wrap: nowrap;[\s\S]*\.msg-activity-meta[\s\S]*width: auto;/,
  );
});
