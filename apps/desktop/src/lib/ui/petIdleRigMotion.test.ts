import assert from "node:assert/strict";
import test from "node:test";
import {
  advanceRigGaze,
  fingerprintPixels,
  idleRigPose,
  idleRigWakeMs,
  restingGaze,
  usesIdleRig,
} from "./petIdleRigMotion.ts";

test("idle rigs never intercept full-body APNG actions or native locomotion", () => {
  assert.equal(usesIdleRig("idle"), true);
  assert.equal(usesIdleRig("look", "idle"), true);
  assert.equal(usesIdleRig("idle", "kneading"), false);
  assert.equal(usesIdleRig("running-right"), false);
  assert.equal(usesIdleRig("idle", undefined, "grooming"), false);
  assert.equal(usesIdleRig("idle", undefined, undefined, 0), false);
});
test("gaze is bounded, frame-rate independent and reverses from the current pose", () => {
  const once = advanceRigGaze(restingGaze(), 90, 32);
  const twice = advanceRigGaze(advanceRigGaze(restingGaze(), 90, 16), 90, 16);
  assert.ok(Math.abs(once.headX - twice.headX) < 0.00001);
  let pose = restingGaze();
  for (let i = 0; i < 100; i++) pose = advanceRigGaze(pose, 90, 16);
  assert.ok(pose.headX <= 1.6 && pose.eyeX <= 1.1);
  const reversed = advanceRigGaze(pose, 270, 16);
  assert.ok(reversed.headX > 0 && reversed.headX < pose.headX);
  for (let i = 0; i < 100; i++) pose = advanceRigGaze(pose, null, 16);
  assert.deepEqual(pose, restingGaze());
});
test("blinks, tail and ears have rest intervals and finite bounded local motion", () => {
  assert.equal(idleRigWakeMs(400), 4600);
  assert.equal(idleRigWakeMs(100), 32);
  assert.deepEqual(idleRigPose(400, 7), { blink: 0, ear: 0, tail: 0 });
  assert.equal(idleRigPose(100, 7).blink, 8);
  for (let time = 0; time < 30000; time += 16) {
    const pose = idleRigPose(time, 7);
    assert.ok(pose.blink >= 0 && pose.blink <= 8);
    assert.ok(Math.abs(pose.tail) <= 7 && Math.abs(pose.ear) <= 1);
  }
});
test("fingerprinting ignores premultiplication noise but retains opaque identity pixels", () => {
  const bytes = new Uint8ClampedArray([23, 44, 65, 255, 87, 44, 62, 1]);
  assert.deepEqual(
    Array.from(fingerprintPixels(bytes)),
    [23, 44, 65, 255, 0, 0, 0, 0],
  );
  assert.equal(bytes[4], 87);
});
