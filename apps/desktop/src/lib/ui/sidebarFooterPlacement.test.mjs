import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const shellStyles = await readFile(
  new URL("../../styles/features/shell/shell.css", import.meta.url),
  "utf8",
);
const polishStyles = await readFile(
  new URL("../../styles/features/shell/layout/sidebar-polish.css", import.meta.url),
  "utf8",
);

test("sidebar footer divider and preferences action move down together", () => {
  assert.match(shellStyles, /\.sidebar\s*\{[\s\S]*?padding:\s*44px 8px 8px;/);
  assert.match(
    shellStyles,
    /\.sidebar\.is-labels\s*\{[\s\S]*?padding:\s*44px 10px 8px;/,
  );
  assert.match(
    polishStyles,
    /\.sidebar-footer\s*\{[\s\S]*?min-height:\s*46px;[\s\S]*?padding:\s*6px 0 0;/,
  );
});
