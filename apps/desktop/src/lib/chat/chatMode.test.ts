import assert from "node:assert/strict";
import test from "node:test";

import {
  normalizeStoredChatMode,
  shouldAutoApproveModeSwitch,
} from "./chatMode.ts";

test("legacy multitask mode migrates to agent", () => {
  assert.equal(normalizeStoredChatMode("multitask"), "agent");
  assert.equal(normalizeStoredChatMode("ask"), "agent");
  assert.equal(normalizeStoredChatMode("agent"), "agent");
  assert.equal(normalizeStoredChatMode("plan"), "plan");
});

test("only Agent to Plan switches automatically", () => {
  assert.equal(
    shouldAutoApproveModeSwitch({ to: "plan", reason: "complex task" }),
    true,
  );
  assert.equal(
    shouldAutoApproveModeSwitch({
      to: "agent",
      reason: "plan ready",
      summary: "step 1",
    }),
    false,
  );
});
