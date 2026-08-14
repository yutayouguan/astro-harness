import assert from "node:assert/strict";
import { test } from "node:test";
import {
  defaultThinkingLevelFromMeta,
  thinkingLevelsFromMeta,
} from "./thinkingPrefs.ts";

test("thinkingLevelsFromMeta includes off unless mandatory", () => {
  assert.deepEqual(
    thinkingLevelsFromMeta({
      supported_efforts: ["xhigh", "high"],
      mandatory: false,
    }),
    ["off", "high", "xhigh"],
  );
  assert.deepEqual(
    thinkingLevelsFromMeta({
      supported_efforts: ["high", "xhigh"],
      mandatory: true,
    }),
    ["high", "xhigh"],
  );
});

test("defaultThinkingLevelFromMeta respects default_enabled and default_effort", () => {
  assert.equal(
    defaultThinkingLevelFromMeta({
      supported_efforts: ["xhigh", "high"],
      default_effort: "high",
      default_enabled: true,
    }),
    "high",
  );
  assert.equal(
    defaultThinkingLevelFromMeta({
      supported_efforts: ["high", "medium", "low"],
      default_enabled: false,
    }),
    "off",
  );
  assert.equal(
    defaultThinkingLevelFromMeta({
      supported_efforts: ["high"],
      mandatory: true,
      default_effort: "high",
    }),
    "high",
  );
});
