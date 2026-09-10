import assert from "node:assert/strict";
import test from "node:test";
import {
  motionDuration,
  motionFrame,
  motionSequence,
  type PetMotionClip,
} from "./petMotionClip.ts";

const clip: PetMotionClip = {
  path: "motion.webp",
  frameWidth: 192,
  frameHeight: 208,
  columns: 4,
  durationsMs: Array(17).fill(90),
  loopStart: 5,
  loopEnd: 13,
  loopRepeats: 3,
};
test("motion clip plays entry once, repeats only the loop, then exits once", () => {
  const sequence = motionSequence(clip);
  assert.equal(sequence.length, 33);
  assert.deepEqual(sequence.slice(0, 5), [0, 1, 2, 3, 4]);
  assert.deepEqual(sequence.slice(13, 21), [5, 6, 7, 8, 9, 10, 11, 12]);
  assert.deepEqual(sequence.slice(-4), [13, 14, 15, 16]);
  assert.equal(motionDuration(clip), 2970);
});
test("arbitrary clip rows finish on the resting pose without restarting entry", () => {
  assert.deepEqual(motionFrame(clip, 2970), { row: 4, column: 0, done: true });
  assert.deepEqual(motionFrame(clip, 2970, false, true), {
    row: 0,
    column: 0,
    done: false,
  });
  assert.equal(motionFrame(clip, 400, true).column, 0);
});
