import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const shellStyles = await readFile(
  new URL("../../styles/features/shell/header.css", import.meta.url),
  "utf8",
);

test("feature toolbars remain interactive above the native drag region", () => {
  assert.match(
    shellStyles,
    /\.feature-content-inline :is\(\.cron-toolbar, \.loop-toolbar, \.plugins-command-bar\)\s*\{[\s\S]*?position:\s*relative;[\s\S]*?z-index:\s*45;[\s\S]*?pointer-events:\s*none;/,
  );
  assert.match(
    shellStyles,
    /\.feature-content-inline :is\(\.cron-toolbar, \.loop-toolbar, \.plugins-command-bar\) > \*\s*\{[\s\S]*?pointer-events:\s*auto;/,
  );
});
