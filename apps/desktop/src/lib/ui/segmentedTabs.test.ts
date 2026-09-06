import { test } from "node:test";
import assert from "node:assert/strict";
import { nextEnabledTabIndex } from "./segmentedTabs.ts";

const disabled = [false, true, false, false] as const;

test("ArrowRight skips disabled tabs and wraps", () => {
  assert.equal(nextEnabledTabIndex(0, "ArrowRight", disabled), 2);
  assert.equal(nextEnabledTabIndex(3, "ArrowRight", disabled), 0);
});

test("ArrowLeft skips disabled tabs and wraps", () => {
  assert.equal(nextEnabledTabIndex(2, "ArrowLeft", disabled), 0);
  assert.equal(nextEnabledTabIndex(0, "ArrowLeft", disabled), 3);
});

test("Home and End select boundary enabled tabs", () => {
  assert.equal(nextEnabledTabIndex(2, "Home", disabled), 0);
  assert.equal(nextEnabledTabIndex(0, "End", disabled), 3);
});

test("navigation returns -1 when every tab is disabled", () => {
  assert.equal(nextEnabledTabIndex(0, "ArrowRight", [true, true]), -1);
});
