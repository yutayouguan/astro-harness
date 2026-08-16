import assert from "node:assert/strict";
import test from "node:test";

import {
  normalizeStoredChatMode,
  resolveInteractionMode,
  resolveStoredParallelTasks,
} from "./chatMode.ts";

test("legacy multitask mode migrates to agent with parallel tasks enabled", () => {
  assert.equal(normalizeStoredChatMode("multitask"), "agent");
  assert.equal(resolveStoredParallelTasks("multitask", null), true);
});

test("explicit parallel task preference wins over the legacy mode", () => {
  assert.equal(resolveStoredParallelTasks("multitask", "false"), false);
  assert.equal(resolveStoredParallelTasks("agent", "true"), true);
});

test("only Agent mode can resolve to the multitask runtime mode", () => {
  assert.equal(resolveInteractionMode("agent", true), "multitask");
  assert.equal(resolveInteractionMode("plan", true), "plan");
  assert.equal(resolveInteractionMode("ask", true), "ask");
});
