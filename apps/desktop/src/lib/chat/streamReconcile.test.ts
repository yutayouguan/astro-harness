import assert from "node:assert/strict";
import { test } from "node:test";

import { reconcileAssistantText } from "./streamReconcile.ts";

test("canonical snapshot replaces divergent streamed assistant text", () => {
  assert.equal(reconcileAssistantText("hel world", "hello world"), "hello world");
});
