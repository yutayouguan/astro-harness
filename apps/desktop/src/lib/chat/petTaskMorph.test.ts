import assert from "node:assert/strict";
import test from "node:test";
import { advanceMorphSpring } from "./petTaskMorph.ts";
test("preview spring produces intermediate frames and converges without bounce", () => {
  let value = 0,
    velocity = 0,
    previous = 0;
  for (let i = 0; i < 120; i++) {
    ({ value, velocity } = advanceMorphSpring(value, velocity, 1, 1 / 60));
    assert.ok(value >= previous && value <= 1);
    previous = value;
    if (i === 0) assert.ok(value > 0 && value < 1);
  }
  assert.ok(1 - value < 0.001);
});
test("reversal starts from the live value and preserves instantaneous velocity", () => {
  const opening = advanceMorphSpring(0.4, 2, 1, 0.016);
  assert.deepEqual(
    advanceMorphSpring(opening.value, opening.velocity, 0, 0),
    opening,
  );
  const next = advanceMorphSpring(opening.value, opening.velocity, 0, 0.016);
  assert.ok(Math.abs(next.value - opening.value) < 0.1);
});
