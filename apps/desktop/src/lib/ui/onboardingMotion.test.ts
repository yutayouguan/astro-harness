import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ONBOARDING_WARP_MS,
  onboardingStageVariants,
} from "./onboardingMotion.ts";

test("forward and backward steps travel consistently through depth", () => {
  const variants = onboardingStageVariants(false);
  assert.equal(typeof variants.enter, "function");
  assert.equal(typeof variants.exit, "function");
  if (
    typeof variants.enter !== "function" ||
    typeof variants.exit !== "function"
  )
    return;
  assert.equal(variants.enter(1).transform, variants.exit(-1).transform);
  assert.equal(variants.enter(-1).transform, variants.exit(1).transform);
  assert.match(variants.enter(1).transform, /-1100px/);
  assert.match(variants.exit(1).transform, /\(650px\)/);
  assert.equal(ONBOARDING_WARP_MS.step, 760);
  assert.ok(ONBOARDING_WARP_MS.app < 1200);
});

test("reduced motion has no scale or spatial movement", () => {
  const variants = onboardingStageVariants(true);
  for (const value of Object.values(variants)) {
    assert.equal(typeof value, "object");
    assert.ok(!("transform" in value));
    assert.ok(!("x" in value));
    assert.ok(!("scale" in value));
  }
  assert.equal(ONBOARDING_WARP_MS.reduced, 160);
});
