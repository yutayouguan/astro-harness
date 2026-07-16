import { test } from "node:test";
import assert from "node:assert/strict";
import { mergeInitialFieldValues } from "./initialFieldValues.ts";

test("fills an empty field from initial values", () => {
  assert.deepEqual(
    mergeInitialFieldValues({ city: "" }, { city: "Hangzhou" }),
    { city: "Hangzhou" },
  );
});

test("does not overwrite user input", () => {
  assert.deepEqual(
    mergeInitialFieldValues({ city: "Shanghai" }, { city: "Hangzhou" }),
    { city: "Shanghai" },
  );
});

test("keeps unrelated fields", () => {
  assert.deepEqual(
    mergeInitialFieldValues({ note: "x" }, { city: "Hangzhou" }),
    { note: "x", city: "Hangzhou" },
  );
});
