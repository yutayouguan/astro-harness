import assert from "node:assert/strict";
import test from "node:test";

import {
  frameForElapsed,
  frameForLookAngle,
  resolveDesktopPetActivity,
} from "./desktopPetAnimation.ts";

test("animation rows follow the Codex v2 contract", () => {
  assert.deepEqual(frameForElapsed("idle", 0), { row: 0, column: 0 });
  assert.deepEqual(frameForElapsed("idle", 280), { row: 0, column: 0 });
  assert.deepEqual(frameForElapsed("idle", 3200), { row: 0, column: 1 });
  assert.deepEqual(frameForElapsed("running", 600), { row: 7, column: 5 });
  assert.deepEqual(frameForElapsed("failed", 9999, true), {
    row: 5,
    column: 0,
  });
});

test("look directions advance clockwise across the two v2 rows", () => {
  assert.deepEqual(frameForLookAngle(0), { row: 9, column: 0 });
  assert.deepEqual(frameForLookAngle(90), { row: 9, column: 4 });
  assert.deepEqual(frameForLookAngle(180), { row: 10, column: 0 });
  assert.deepEqual(frameForLookAngle(270), { row: 10, column: 4 });
  assert.deepEqual(frameForLookAngle(337.5), { row: 10, column: 7 });
});

test("waiting outranks running in aggregate session activity", () => {
  assert.equal(
    resolveDesktopPetActivity({
      a: { status: "active", activeFlags: [], updatedAt: 1 },
      b: {
        status: "active",
        activeFlags: ["waitingOnApproval"],
        updatedAt: 2,
      },
    }),
    "waiting",
  );
  assert.equal(
    resolveDesktopPetActivity({
      a: { status: "active", activeFlags: [], updatedAt: 1 },
    }),
    "running",
  );
});
