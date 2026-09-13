import test from "node:test";
import assert from "node:assert/strict";
import { createPetPlaybackClock } from "./petPlaybackClock.ts";

test("pause freezes current phase and resume excludes elapsed wall time", () => {
  const clock = createPetPlaybackClock();
  assert.equal(clock.sample(100, false), 0);
  assert.equal(clock.sample(150, false), 50);
  assert.equal(clock.sample(170, true), 50);
  assert.equal(clock.sample(9000, true), 50);
  assert.equal(clock.sample(10000, false), 50);
  assert.equal(clock.sample(10040, false), 90);
  clock.reset();
  assert.equal(clock.sample(20000, false), 0);
});

test("initial pause and non-finite clocks do not advance playback", () => {
  const clock = createPetPlaybackClock();
  assert.equal(clock.sample(100, true), 0);
  assert.equal(clock.sample(Infinity, false), 0);
  assert.equal(clock.sample(20000, false), 0);
  assert.equal(clock.sample(19990, false), 0);
});
