import assert from "node:assert/strict";
import { test } from "node:test";
import {
  formatCompactTurnTokens,
  formatExactTurnTokens,
  formatTurnDuration,
} from "./turnUsageDisplay.ts";

test("turn usage numbers follow the active locale", () => {
  assert.equal(formatCompactTurnTokens(199_333, "zh"), "19.9万");
  assert.equal(formatCompactTurnTokens(199_333, "en"), "199.3K");
  assert.equal(formatExactTurnTokens(10_683, "zh"), "10,683");
  assert.equal(formatExactTurnTokens(Number.NaN, "en"), "0");
});

test("turn duration becomes human-readable after one minute", () => {
  assert.equal(formatTurnDuration(4.84, "zh"), "4.8秒");
  assert.equal(formatTurnDuration(258, "zh"), "4分18秒");
  assert.equal(formatTurnDuration(258, "en"), "4m 18s");
  assert.equal(formatTurnDuration(300, "zh"), "5分钟");
});
