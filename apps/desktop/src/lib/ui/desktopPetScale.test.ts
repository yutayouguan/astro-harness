import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  DESKTOP_PET_SCALE,
  petScalePercent,
  petScaleFromPercent,
  EMPTY_DESKTOP_PET_STATE,
} from "./desktopPetState.ts";

test("slider allows less than half the old minimum and matches persistent native bounds", () => {
  assert.ok(DESKTOP_PET_SCALE.min <= 0.65 / 2);
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
  ] as const) {
    const match = source.match(
      new RegExp("DESKTOP_PET_" + symbol + "_SCALE: f64 = ([0-9.]+)"),
    );
    assert.ok(match);
    assert.equal(DESKTOP_PET_SCALE[field], Number(match[1]));
  }
});

test("new 100 percent means the old 40 percent, with a 150 percent maximum", () => {
  assert.equal(petScalePercent(0.4), 100);
  assert.equal(petScalePercent(0.6), 150);
  assert.equal(petScalePercent(0.3), 75);
  assert.equal(petScalePercent(0.35), 87.5);
  assert.equal(EMPTY_DESKTOP_PET_STATE.scale, 0.4);
  assert.equal(petScaleFromPercent(100), 0.4);
  assert.equal(petScaleFromPercent(150), 0.6);
  assert.equal(petScaleFromPercent(1000), 0.6);
});
