import test from "node:test";
import assert from "node:assert/strict";
import {
  acceptRoamingFrame,
  roamingInterval,
  supportsPetRoaming,
  type PetRoamingFrame,
} from "./petRoaming.ts";

test("late native frames cannot revive a cancelled walk or reverse its phase", () => {
  const active: PetRoamingFrame = {
    generation: 4,
    petId: "a",
    revision: 10,
    active: true,
    returning: false,
    clipName: "running-right",
    clip: null,
    elapsedMs: 100,
  };
  assert.equal(
    acceptRoamingFrame(active, { ...active, elapsedMs: 20 }),
    active,
  );
  const stopped = { ...active, generation: 5, active: false };
  assert.equal(acceptRoamingFrame(stopped, active), stopped);
  assert.equal(
    acceptRoamingFrame(stopped, { ...active, generation: 5, elapsedMs: 200 }),
    stopped,
  );
});
test("roaming requires two APNG walks with declared stride, not merely animated artwork", () => {
  const clip = {
    path: "walk.apng",
    frameWidth: 192,
    frameHeight: 208,
    columns: 1,
    durationsMs: [100, 100, 100, 100, 100, 100],
    loopStart: 1,
    loopEnd: 5,
    loopRepeats: 1,
    locomotion: { stridePx: 80 },
  };
  assert.equal(supportsPetRoaming(undefined), false);
  assert.equal(supportsPetRoaming({ "running-right": clip }), false);
  assert.equal(
    supportsPetRoaming({
      "running-right": clip,
      "running-left": { ...clip, path: "old.webp" },
    }),
    false,
  );
  assert.equal(
    supportsPetRoaming({ "running-right": clip, "running-left": clip }),
    true,
  );
  assert.equal(roamingInterval(45, 0.5), 90000);
  for (const invalid of [
    { ...clip, loopStart: 0 },
    { ...clip, loopEnd: 6 },
    { ...clip, durationsMs: [100, 50, 50, 50, 50, 100] },
    { ...clip, locomotion: { stridePx: 385 } },
    { ...clip, loopStart: 2 },
    { ...clip, durationsMs: [2100, 100, 100, 100, 100, 100] },
  ]) assert.equal(supportsPetRoaming({ "running-left": clip, "running-right": invalid }), false);
});
