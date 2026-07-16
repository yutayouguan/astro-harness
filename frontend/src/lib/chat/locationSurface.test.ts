import { test } from "node:test";
import assert from "node:assert/strict";
import { isLocationRequiredSurface } from "./locationSurface.ts";
import type { UiSurface } from "../../types.ts";

const base: UiSurface = {
  messageId: "m1",
  activityType: "a2ui-surface",
  operations: [],
  status: "active",
};

test("detects an active location interrupt", () => {
  assert.equal(
    isLocationRequiredSurface({
      ...base,
      interrupts: [{ id: "i1", reason: "location_required" }],
    }),
    true,
  );
});

test("rejects resolved and unrelated surfaces", () => {
  assert.equal(
    isLocationRequiredSurface({
      ...base,
      status: "resolved",
      interrupts: [{ id: "i1", reason: "location_required" }],
    }),
    false,
  );
  assert.equal(
    isLocationRequiredSurface({
      ...base,
      interrupts: [{ id: "i2", reason: "confirmation_required" }],
    }),
    false,
  );
});
