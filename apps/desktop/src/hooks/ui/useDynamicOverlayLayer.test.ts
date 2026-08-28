import assert from "node:assert/strict";
import test from "node:test";
import {
  claimOverlayLayer,
  nextOverlayLayer,
} from "./useDynamicOverlayLayer.ts";

test("each claimed app overlay layer is above the previous one", () => {
  const first = claimOverlayLayer();
  const second = claimOverlayLayer();
  const third = claimOverlayLayer();

  assert.ok(second > first);
  assert.ok(third > second);
});

test("reserved browser layers do not overflow the app overlay stack", () => {
  assert.equal(nextOverlayLayer(1200, [1400, 2_147_483_647]), 1401);
});
