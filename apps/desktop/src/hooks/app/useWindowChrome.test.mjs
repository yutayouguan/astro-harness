import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const hookSource = await readFile(
  new URL("./useWindowChrome.ts", import.meta.url),
  "utf8",
);

test("native window dragging starts in the initiating mouse-down handler", () => {
  const handler = hookSource.match(
    /const onTitleMouseDown = [\s\S]*?(?=\n  const onTitleDoubleClick)/,
  )?.[0];

  assert.ok(handler, "title mouse-down handler should exist");
  assert.match(handler, /\.startDragging\(\)/);
  assert.doesNotMatch(
    handler,
    /setTimeout/,
    "startDragging must not be deferred past the initiating mouse-down event",
  );
  assert.match(
    handler,
    /\.catch\(\(error\) => \{\s*console\.warn\("start window dragging failed", error\);/,
    "native drag failures should remain observable",
  );
});
