import assert from "node:assert/strict";
import { test } from "node:test";

import {
  consumeBufferedTextReconcile,
  reconcileAssistantText,
} from "./streamReconcile.ts";

test("canonical snapshot replaces divergent streamed assistant text", () => {
  assert.equal(reconcileAssistantText("hel world", "hello world"), "hello world");
});

test("buffered parallel draft is consumed by canonical reconciliation", () => {
  assert.deepEqual(
    consumeBufferedTextReconcile("hel", " world", "hello world"),
    { content: "hello world", buffered: "" },
  );
});
