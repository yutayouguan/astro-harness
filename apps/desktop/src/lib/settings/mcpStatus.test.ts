import assert from "node:assert/strict";
import test from "node:test";
import { mcpDisplayState } from "./mcpStatus.ts";
import type { McpRuntimeState } from "../../hooks/providers/useMcpTools.ts";

test("disabled configuration always has a disabled indicator", () => {
  assert.equal(mcpDisplayState(false), "disabled");
  assert.equal(mcpDisplayState(false, { status: "connected" }), "disabled");
  assert.equal(mcpDisplayState(false, { status: "error" }, true), "disabled");
});

test("missing runtime data never implies a successful connection", () => {
  assert.equal(mcpDisplayState(true), "unknown");
  assert.equal(mcpDisplayState(true, { status: "error" }, true), "connecting");
});

test("runtime states retain their distinct indicator semantics", () => {
  const states: McpRuntimeState[] = [
    "configured",
    "disabled",
    "connecting",
    "connected",
    "disconnected",
    "backoff",
    "auth-required",
    "error",
    "unknown",
  ];
  for (const state of states)
    assert.equal(mcpDisplayState(true, { status: state }), state);
});
