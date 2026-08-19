import assert from "node:assert/strict";
import test from "node:test";

import {
  normalizeStoredChatMode,
} from "./chatMode.ts";

test("legacy multitask mode migrates to agent", () => {
  assert.equal(normalizeStoredChatMode("multitask"), "agent");
  assert.equal(normalizeStoredChatMode("agent"), "agent");
});
