import assert from "node:assert/strict";
import test from "node:test";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import {
  composeNaitangBlink,
  createPetBlinkTimeline,
  naitangBlinkProfile,
  NAITANG_EYES,
  petBlinkClosure,
  PET_BLINK_DURATION,
} from "./desktopPetBlink.ts";

test("blink closes for 70ms, holds for 80ms, then opens over 110ms", () => {
  assert.equal(PET_BLINK_DURATION, 260);
  assert.equal(petBlinkClosure(-1), 0);
  assert.equal(petBlinkClosure(0), 0);
  assert.equal(petBlinkClosure(35), 0.5);
  assert.equal(petBlinkClosure(70), 1);
  assert.equal(petBlinkClosure(149), 1);
  assert.equal(petBlinkClosure(205), 0.5);
  assert.equal(petBlinkClosure(260), 0);
});

test("random intervals stay between 5 and 8 seconds without randomizing each frame", () => {
  for (const [random, gap] of [
    [0, 5000],
    [1, 8000],
  ] as const) {
    let calls = 0;
    const clock = createPetBlinkTimeline(() => {
      calls++;
      return random;
    });
    for (let elapsed = 0; elapsed < gap; elapsed += 16)
      assert.equal(clock.sample(elapsed), 0);
    assert.equal(calls, 1);
    assert.equal(clock.sample(gap + 35), 0.5);
    assert.equal(clock.sample(gap + 260), 0);
    assert.equal(clock.sample(gap * 2 + 35), 0.5);
  }
});

test("resuming after suspension skips missed blinks instead of bursting", () => {
  const clock = createPetBlinkTimeline(() => 0);
  assert.equal(clock.sample(60000), 0);
  assert.equal(clock.sample(60001), 0);
  assert.equal(clock.sample(65035), 0.5);
  assert.equal(clock.sample(0), 0);
  assert.equal(clock.sample(5035), 0.5);
});

test("intermediate eyelids never change head, body, silhouette or alpha", () => {
  const neutral = new Uint8ClampedArray(192 * 208 * 4);
  const closed = new Uint8ClampedArray(neutral.length);
  for (let at = 0; at < neutral.length; at += 4) {
    neutral.set([20, 40, 60, 255], at);
    closed.set([200, 120, 80, 255], at);
  }
  assert.deepEqual(composeNaitangBlink(neutral, closed, 0), neutral);
  for (const closure of [0.25, 0.5, 0.75, 1]) {
    const result = composeNaitangBlink(neutral, closed, closure);
    let changed = 0;
    for (let y = 0; y < 208; y++)
      for (let x = 0; x < 192; x++) {
        const at = (y * 192 + x) * 4;
        const inEye = NAITANG_EYES.some(
          (eye) => Math.hypot((x - eye.x) / eye.rx, (y - eye.y) / eye.ry) < 1,
        );
        assert.equal(result[at + 3], neutral[at + 3]);
        if (!inEye)
          assert.deepEqual(result.slice(at, at + 4), neutral.slice(at, at + 4));
        if (result[at] !== neutral[at]) changed++;
      }
    assert.ok(changed > 0);
  }
});

test("calibrated eye masks apply only to the matching bundled Naitang artwork", () => {
  assert.equal(
    naitangBlinkProfile("/owned/builtin-naitang-v2/spritesheet.webp"),
    "naitang",
  );
  assert.equal(
    naitangBlinkProfile("C:\\owned\\builtin-naitang-v2\\spritesheet.webp"),
    "naitang",
  );
  assert.equal(
    naitangBlinkProfile("/owned/custom-pet/spritesheet.webp"),
    undefined,
  );
  const bytes = readFileSync(
    new URL("../../assets/pets/naitang/spritesheet.webp", import.meta.url),
  );
  assert.equal(
    createHash("sha256").update(bytes).digest("hex"),
    "38ee5414b6bb8b1deb08f488e357c7bb7962428cb14e628491894a28199b85e5",
    "Recalibrate the eye masks when changing the base artwork",
  );
});
