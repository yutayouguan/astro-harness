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
  assert.equal(variants.enter(1).transform, variants.exit(-1).transform.at(-1));
  assert.equal(variants.enter(-1).transform, variants.exit(1).transform.at(-1));
  assert.match(variants.enter(1).transform, /-650px/);
  assert.match(variants.exit(1).transform.at(-1)!, /\(500px\)/);
  assert.equal(ONBOARDING_WARP_MS.step, 1100);
  assert.equal(ONBOARDING_WARP_MS.app, 1400);
});

test("resting pages release perspective and exits restart without a jump", () => {
  for (const reduced of [false, true]) {
    assert.equal(
      onboardingStageVariants(reduced).center.transitionEnd.transform,
      "none",
    );
  }
  const variants = onboardingStageVariants(false);
  if (typeof variants.exit !== "function") throw Error("Missing depth exit");
  assert.equal(
    variants.exit(1, { transform: "none" }).transform[0],
    "perspective(1200px) translateZ(0px)",
  );
  const interrupted = "perspective(1200px) translateZ(-180px)";
  assert.equal(
    variants.exit(-1, { transform: interrupted }).transform[0],
    interrupted,
  );
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
