import test from "node:test";
import assert from "node:assert/strict";
import {
  acceptRoamingFrame,
  roamingInterval,
  supportsPetRoaming,
  withGroundPlacement,
  type PetRoamingFrame,
} from "./petRoaming.ts";
import { DEFAULT_PET_PREFERENCES } from "./petPreferences.ts";

test("ground placement is saved into the draft without replacing other unsaved preferences", () => {
  const draft = { scale: 0.2, behavior: { ...DEFAULT_PET_PREFERENCES, activityIntervalSecs: 90, quietMode: true } };
  const position = { monitor: "second", monitorX: -1000, monitorY: 0, x: 0.6, y: 1 };
  const placed = { ...DEFAULT_PET_PREFERENCES, position, roamingEnabled: true };
  const result = withGroundPlacement(draft, placed);
  assert.equal(result.scale, 0.2);
  assert.equal(result.behavior.activityIntervalSecs, 90);
  assert.equal(result.behavior.quietMode, true);
  assert.equal(result.behavior.roamingEnabled, true);
  assert.deepEqual(result.behavior.position, position);
  assert.notEqual(result.behavior.position, position);
  assert.equal(draft.behavior.position, null);
  assert.throws(() => withGroundPlacement(draft, undefined));
  assert.throws(() => withGroundPlacement(draft, { ...placed, roamingEnabled: false }));
});

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
