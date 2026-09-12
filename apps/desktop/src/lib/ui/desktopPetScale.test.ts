import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  DESKTOP_PET_SCALE,
  petScalePercent,
  petScaleFromPercent,
  EMPTY_DESKTOP_PET_STATE,
} from "./desktopPetState.ts";

test("slider reaches half the rebased default and matches persistent native bounds", () => {
  assert.equal(DESKTOP_PET_SCALE.min, DESKTOP_PET_SCALE.reference / 2);
  assert.equal(DESKTOP_PET_SCALE.max, 0.6);
  assert.equal(DESKTOP_PET_SCALE.step, 0.01);
  const source = readFileSync(
    new URL(
      "../../../../../crates/agent-types/src/desktop_pet.rs",
      import.meta.url,
    ),
    "utf8",
  );
  for (const [field, symbol] of [
    ["min", "MIN"],
    ["max", "MAX"],
    ["reference", "DEFAULT"],
  ] as const) {
    const match = source.match(
      new RegExp("DESKTOP_PET_" + symbol + "_SCALE: f64 = ([0-9.]+)"),
    );
    assert.ok(match);
    assert.equal(DESKTOP_PET_SCALE[field], Number(match[1]));
  }
});

test("old 75 percent becomes 100 percent without rescaling stored values", () => {
  assert.equal(petScalePercent(0.3), 100);
  assert.equal(petScalePercent(0.15), 50);
  assert.equal(petScalePercent(0.4), 133.3);
  assert.equal(petScalePercent(0.6), 200);
  assert.equal(EMPTY_DESKTOP_PET_STATE.scale, 0.3);
  assert.equal(petScaleFromPercent(50), 0.15);
  assert.equal(petScaleFromPercent(100), 0.3);
  assert.equal(petScaleFromPercent(150), 0.45);
  assert.equal(petScaleFromPercent(200), 0.6);
  assert.equal(petScaleFromPercent(1000), 0.6);
  assert.equal(petScaleFromPercent(0), 0.15);
  assert.equal(petScaleFromPercent(Number.NaN), 0.3);
});
