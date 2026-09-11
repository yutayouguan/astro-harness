import assert from "node:assert/strict";
import test from "node:test";
import {
  availablePetActions,
  availablePetActionSignature,
  nextPetAction,
  petActionLabel,
} from "./petActionCatalog.ts";
import {
  canPlayPetLeisure,
  legacyPetLeisureDuration,
} from "./desktopPetLeisure.ts";
import type { PetMotionClip } from "./petMotionClip";

const clip: PetMotionClip = {
  path: "motion.webp",
  frameWidth: 192,
  frameHeight: 208,
  columns: 4,
  durationsMs: Array(24).fill(100),
  loopStart: 6,
  loopEnd: 18,
  loopRepeats: 2,
  neutralBookends: true,
};
const dog = {
  spriteVersionNumber: 2,
  groomingPath: null,
  motionClips: {
    "tail-wag": clip,
    "head-tilt": clip,
    stretch: clip,
    nap: clip,
  },
};
test("Pudding exposes only its own canine motions with shared bilingual labels", () => {
  assert.deepEqual(availablePetActions(dog), [
    "tail-wag",
    "head-tilt",
    "stretch",
    "nap",
  ]);
  assert.equal(petActionLabel("tail-wag", "zh"), "摇尾巴");
  assert.equal(petActionLabel("nap", "en"), "Nap");
  assert.equal(legacyPetLeisureDuration("tail-wag"), 0);
  assert.ok(!availablePetActions(dog).includes("kneading"));
});
test("Naitang and old grooming packages keep their real supported actions", () => {
  assert.deepEqual(
    availablePetActions({
      spriteVersionNumber: 2,
      motionClips: { grooming: clip, kneading: clip },
    }),
    ["kneading", "grooming"],
  );
  assert.deepEqual(
    availablePetActions({
      spriteVersionNumber: 2,
      groomingPath: "legacy.webp",
    }),
    ["kneading", "grooming"],
  );
  assert.deepEqual(
    availablePetActions({ ...dog, spriteVersionNumber: null }),
    [],
  );
});
test("auto-play cycles available clips and safely resets after switching pet", () => {
  const available = availablePetActions(dog);
  assert.equal(nextPetAction(available, "kneading"), "tail-wag");
  assert.equal(nextPetAction(available, "tail-wag"), "head-tilt");
  assert.equal(nextPetAction(available, "nap"), "tail-wag");
  assert.equal(nextPetAction([], "tail-wag"), null);
});
test("imported custom clips participate without adding a new species schema", () => {
  const source = { spriteVersionNumber: 2, motionClips: { "happy-hop": clip } };
  assert.deepEqual(availablePetActions(source), ["happy-hop"]);
  assert.equal(petActionLabel("happy-hop", "zh"), "happy-hop");
  assert.notEqual(
    availablePetActionSignature(source),
    availablePetActionSignature({
      ...source,
      motionClips: { "happy-hop": { ...clip, path: "new.webp" } },
    }),
  );
});
test("dog activity respects visibility quiet pause dragging and reduced-motion gates", () => {
  const input = {
    enabled: true,
    spriteVersionNumber: 2,
    groomingPath: null,
    hasMotionClips: availablePetActions(dog).length > 0,
    quietMode: false,
    paused: false,
    reducedMotion: false,
    activity: "idle",
    dragging: false,
  };
  assert.equal(canPlayPetLeisure(input), true);
  for (const change of [
    { enabled: false },
    { quietMode: true },
    { paused: true },
    { reducedMotion: true },
    { activity: "running" },
    { dragging: true },
  ])
    assert.equal(canPlayPetLeisure({ ...input, ...change }), false);
});
