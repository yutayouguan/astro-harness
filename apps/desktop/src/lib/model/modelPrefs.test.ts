import assert from "node:assert/strict";
import { test } from "node:test";
import {
  clampPrefsToModelConfig,
  contextChoicesForWindow,
  effortChoicesFromMeta,
  modelSupportsReasoning,
  type ModelRuntimePrefs,
} from "./modelPrefs.ts";

const basePrefs: ModelRuntimePrefs = {
  thinking: true,
  fast: true,
  context: "1m",
  effort: "xhigh",
};

test("contextChoicesForWindow only offers sizes within window", () => {
  assert.deepEqual(contextChoicesForWindow(128_000), []);
  assert.deepEqual(contextChoicesForWindow(300_000), ["300k"]);
  assert.deepEqual(contextChoicesForWindow(1_048_576), ["300k", "1m"]);
  assert.deepEqual(contextChoicesForWindow(null), []);
});

test("effortChoicesFromMeta uses supported_efforts when present", () => {
  assert.deepEqual(
    effortChoicesFromMeta(
      { supported_efforts: ["low", "high"], mandatory: true },
      true,
    ),
    ["low", "high"],
  );
  assert.deepEqual(effortChoicesFromMeta(null, false), []);
  assert.deepEqual(effortChoicesFromMeta(null, true), ["low", "high", "max"]);
});

test("clampPrefsToModelConfig snaps to real options", () => {
  const next = clampPrefsToModelConfig(basePrefs, {
    capsReasoning: true,
    reasoning: {
      supported_efforts: ["low", "medium", "high"],
      mandatory: true,
    },
    contextWindow: 200_000,
  });
  assert.equal(next.thinking, true);
  assert.equal(next.effort, "high");
  assert.equal(next.context, "default");
  assert.equal(next.fast, false);
  assert.equal(modelSupportsReasoning(false, { mandatory: true }), true);
});
