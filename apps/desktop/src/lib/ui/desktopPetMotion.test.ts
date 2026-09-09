import assert from "node:assert/strict";
import test from "node:test";
import {
  advancePetGaze,
  idlePetFrame,
  shortestAngleDelta,
  stablePetGazeFrame,
} from "./desktopPetMotion.ts";

test("idle holds between brief blinks and varies resting intervals", () => {
  assert.equal(idlePetFrame(3100), 0);
  assert.equal(idlePetFrame(3200), 1);
  assert.equal(idlePetFrame(3280), 2);
  assert.equal(idlePetFrame(3900), 0);
  assert.equal(idlePetFrame(7100), 0);
  assert.equal(idlePetFrame(8500), 1);
});
test("gaze crosses zero by the short arc and bounds catch-up after suspension", () => {
  assert.equal(shortestAngleDelta(350, 10), 20);
  const next = advancePetGaze(350, 10, 16);
  assert.ok(next > 350 && next < 360);
  assert.ok(
    Math.abs(shortestAngleDelta(0, advancePetGaze(0, 180, 30000))) <= 30,
  );
});
test("gaze hysteresis prevents pointer jitter from toggling neighboring sprites", () => {
  assert.equal(stablePetGazeFrame(12, 0), 0);
  assert.equal(stablePetGazeFrame(15, 0), 1);
  assert.equal(stablePetGazeFrame(10, 1), 1);
  assert.equal(stablePetGazeFrame(8, 1), 0);
  assert.equal(stablePetGazeFrame(358, 0), 0);
});
