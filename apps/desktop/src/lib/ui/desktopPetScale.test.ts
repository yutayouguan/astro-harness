import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { DESKTOP_PET_SCALE } from "./desktopPetState.ts";

test("slider allows less than half the old minimum and matches persistent native bounds", () => {
  assert.ok(DESKTOP_PET_SCALE.min <= 0.65 / 2);
  assert.equal(DESKTOP_PET_SCALE.max, 1.35);
  assert.equal(DESKTOP_PET_SCALE.step, 0.05);
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
