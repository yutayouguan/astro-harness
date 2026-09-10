import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_PET_PREFERENCES } from "./petPreferences.ts";
import { canPlayPetLeisure } from "./desktopPetLeisure.ts";
import { motionUsesNeutralFrame, type PetMotionClip } from "./petMotionClip.ts";
import { readFileSync } from "node:fs";

test("quiet mode blocks automatic large actions without disabling the pet", () => {
  const state = {
    enabled: true,
    spriteVersionNumber: 2,
    groomingPath: "groom.webp",
    paused: false,
    reducedMotion: false,
    activity: "idle",
    dragging: false,
  };
  assert.equal(canPlayPetLeisure({ ...state, quietMode: true }), false);
  assert.equal(canPlayPetLeisure({ ...state, quietMode: false }), true);
  assert.equal(DEFAULT_PET_PREFERENCES.activityIntervalSecs, 45);
});
test("built-in motion bookends use exactly the shared neutral pose, not generated lookalikes", () => {
  const clips = JSON.parse(
    readFileSync(
      new URL("../../assets/pets/naitang/motion-clips.json", import.meta.url),
      "utf8",
    ),
  );
  for (const clip of Object.values(clips) as PetMotionClip[]) {
    const last = clip.durationsMs.length - 1;
    assert.equal(motionUsesNeutralFrame(clip, 0, 0), true);
    assert.equal(
      motionUsesNeutralFrame(
        clip,
        Math.floor(last / clip.columns),
        last % clip.columns,
      ),
      true,
    );
    assert.equal(
      motionUsesNeutralFrame(
        clip,
        Math.floor(clip.loopStart / clip.columns),
        clip.loopStart % clip.columns,
      ),
      false,
    );
    assert.equal(
      motionUsesNeutralFrame({ ...clip, neutralBookends: false }, 0, 0),
      false,
    );
  }
});
