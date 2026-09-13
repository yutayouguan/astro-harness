import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { canUndoPetScale, createAmbienceSession } from "./ambienceSession.ts";
const read = (path) => readFile(new URL(path, import.meta.url), "utf8");

test("size undo rejects another pet or an externally changed size", () => {
  const undo = { kind: "pet-scale", petId: "cat", before: 0.3, expected: 0.15 };
  assert.equal(
    canUndoPetScale(undo, { activePetId: "cat", scale: 0.15 }),
    true,
  );
  assert.equal(
    canUndoPetScale(undo, { activePetId: "dog", scale: 0.15 }),
    false,
  );
  assert.equal(
    canUndoPetScale(undo, { activePetId: "cat", scale: 0.2 }),
    false,
  );
  assert.equal(canUndoPetScale(undo, { scale: 0.15 }), false);
  const session = createAmbienceSession();
  session.setUndo(undo);
  const leave = session.subscribe(() => {});
  leave();
  assert.deepEqual(session.getSnapshot().undo, undo);
});

test("pet range streams input updates without stealing native thumb dragging", async () => {
  const source = await read("../../components/ui/AmbiencePetScaleControl.tsx");
  for (const field of ["min", "max", "step"])
    assert.ok(source.includes(field + "={DESKTOP_PET_SCALE." + field + "}"));
  assert.match(source, /aria-valuetext=\{petScalePercent\(draft\)/);
  assert.match(
    source,
    /onChange=\{[^}]*change\(Number\(event.currentTarget.value\)\)/,
  );
  assert.match(source, /queue.enqueue\(\{ petId, scale: value, gestureId \}\)/);
  assert.doesNotMatch(
    source,
    /setPointerCapture|onPointerUp=|disabled \|\| saving/,
  );
  assert.match(source, /window.addEventListener\("pointerup", finish\)/);
  assert.match(source, /disabled=\{!petId \|\| \(disabled && !saving\)\}/);
});

test("slider targets the active desktop pet and a keyed remount discards another pet's draft", async () => {
  const panel = await read("../../components/ui/DesktopAmbienceButton.tsx");
  const hook = await read("../../hooks/app/useDesktopAmbience.ts");
  assert.match(panel, /key=\{state.pet.activePetId \?\? "no-pet"\}/);
  assert.match(panel, /onCommit=\{state.setPetScale\}/);
  assert.match(hook, /current.activePetId !== petId/);
  assert.match(
    hook,
    /pet.mutate\("set_desktop_pet_scale", \{\s*petId,\s*scale,?\s*\}\)/,
  );
  assert.match(hook, /canUndoPetScale\(undo, current\)/);
});
