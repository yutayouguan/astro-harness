import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const cssUrl = new URL(
  "../../styles/features/chat/answer-panel.css",
  import.meta.url,
);

test("reply text starts on the same visual line as its timeline icon", async () => {
  const css = await readFile(cssUrl, "utf8");

  assert.match(
    css,
    /msg-timeline-step\.kind-reply \.msg-timeline-rail \{[\s\S]*?padding-top: 0;/,
  );
  assert.match(
    css,
    /msg-timeline-step\.kind-reply \.msg-md > :first-child \{[\s\S]*?margin-top: 0;/,
  );
});
