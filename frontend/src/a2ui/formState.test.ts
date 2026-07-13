import { test } from "node:test";
import assert from "node:assert/strict";
import {
  isFieldFilled,
  mergeActionContext,
  missingRequiredFields,
} from "./formState.ts";
import type { A2uiComponent } from "./types.ts";

test("event context overrides field defaults", () => {
  const merged = mergeActionContext(
    { value: "from-button" },
    { value: "from-field", other: 1 },
  );
  assert.deepEqual(merged, { value: "from-button", other: 1 });
});

test("empty base keeps fields", () => {
  assert.deepEqual(mergeActionContext(undefined, { a: true }), { a: true });
});

test("isFieldFilled treats blank string as empty", () => {
  assert.equal(isFieldFilled(""), false);
  assert.equal(isFieldFilled("  "), false);
  assert.equal(isFieldFilled("ok"), true);
  assert.equal(isFieldFilled(false), false);
  assert.equal(isFieldFilled(true), true);
});

test("missingRequiredFields lists unfilled required ids", () => {
  const comps = [
    { id: "value", component: "ChoicePicker", required: true },
    { id: "note", component: "TextField", required: true },
    { id: "opt", component: "TextField" },
  ] as A2uiComponent[];
  assert.deepEqual(missingRequiredFields(comps, {}), ["value", "note"]);
  assert.deepEqual(missingRequiredFields(comps, { value: "staging" }), [
    "note",
  ]);
  assert.deepEqual(
    missingRequiredFields(comps, { value: "staging", note: "x" }),
    [],
  );
});
