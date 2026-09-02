import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const rightPanelStyles = await readFile(
  new URL("../../styles/features/chat/right-panel.css", import.meta.url),
  "utf8",
);

test("chat inspector keeps one vertical scrollbar track across tabs", () => {
  assert.match(
    rightPanelStyles,
    /\.chat-right-body\s*\{[\s\S]*?overflow-x:\s*hidden;[\s\S]*?overflow-y:\s*scroll;[\s\S]*?scrollbar-gutter:\s*stable;/,
  );
});
