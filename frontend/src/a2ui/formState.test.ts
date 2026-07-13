import { test } from "node:test";
import assert from "node:assert/strict";
import { mergeActionContext } from "./formState.ts";

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
