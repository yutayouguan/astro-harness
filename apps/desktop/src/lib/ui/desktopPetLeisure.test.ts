import assert from "node:assert/strict";
import test from "node:test";
import {
  canPlayPetLeisure,
  groomingFrame,
  LEISURE_DURATION,
} from "./desktopPetLeisure.ts";

test("paw grooming uses its own six-frame strip and pauses deterministically", () => {
  assert.deepEqual(groomingFrame(0), { row: 0, column: 0 });
  assert.deepEqual(groomingFrame(260), { row: 0, column: 1 });
  assert.deepEqual(groomingFrame(1199), { row: 0, column: 5 });
  assert.deepEqual(groomingFrame(1200), { row: 0, column: 0 });
  assert.equal(groomingFrame(700, true).column, 0);
  assert.equal(groomingFrame(Number.NaN).column, 0);
  assert.equal(LEISURE_DURATION.grooming, 2400);
});

test("leisure never replaces active work, approval, drag, pause, or reduced motion", () => {
  const idle = {
    enabled: true,
    spriteVersionNumber: 2,
    groomingPath: "/owned/grooming.png",
    paused: false,
    reducedMotion: false,
    activity: "idle",
    dragging: false,
  };
  assert.equal(canPlayPetLeisure(idle), true);
  for (const change of [
    { enabled: false },
    { spriteVersionNumber: null },
    { groomingPath: null },
    { paused: true },
    { reducedMotion: true },
    { activity: "waiting" },
    { activity: "running" },
    { activity: "failed" },
    { dragging: true },
  ]) {
    assert.equal(canPlayPetLeisure({ ...idle, ...change }), false);
  }
});
