import { test } from "node:test";
import assert from "node:assert/strict";
import { layoutAnchoredMenu } from "./anchoredMenuLayout.ts";

function rect(left: number, top: number, width: number, height: number) {
  return {
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
  };
}

test("fixedWidth respects content-pane widthCap", () => {
  const r = layoutAnchoredMenu({
    anchorRect: rect(100, 40, 32, 32),
    bounds: { left: 0, top: 0, right: 200, bottom: 400 },
    fixedWidth: 176,
    preferAlign: "end",
    placement: "below",
    pad: 8,
  });
  assert.equal(r.width, 176);
  assert.equal(r.widthCap, 176);
  assert.equal(r.openUp, false);
  assert.ok(r.left + r.width <= 200 - 8);
});

test("fixedWidth shrinks when pane is narrow", () => {
  const r = layoutAnchoredMenu({
    anchorRect: rect(20, 40, 32, 32),
    bounds: { left: 0, top: 0, right: 120, bottom: 400 },
    fixedWidth: 176,
    preferAlign: "start",
    placement: "below",
    pad: 8,
  });
  assert.equal(r.widthCap, 104); // 120 - 16
  assert.equal(r.width, 104);
});

test("measured width clamps to maxWidth and widthCap", () => {
  const r = layoutAnchoredMenu({
    anchorRect: rect(10, 40, 160, 32),
    bounds: { left: 0, top: 0, right: 500, bottom: 800 },
    measured: { width: 400, height: 200 },
    minWidth: 140,
    maxWidth: 280,
    preferAlign: "start",
    placement: "below",
  });
  assert.equal(r.widthCap, 280);
  assert.equal(r.width, 280);
});

test("opens above when below space is tight", () => {
  const r = layoutAnchoredMenu({
    anchorRect: rect(40, 350, 120, 32),
    bounds: { left: 0, top: 0, right: 400, bottom: 400 },
    measured: { width: 180, height: 200 },
    maxHeightCap: 260,
    maxHeightRatio: 1,
    preferAlign: "start",
    placement: "auto",
    gap: 6,
  });
  assert.equal(r.openUp, true);
  assert.ok(r.maxHeight > 0);
});
