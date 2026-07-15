import { test } from "node:test";
import assert from "node:assert/strict";
import { clampPopover } from "./clampPopover.ts";

function rect(
  left: number,
  top: number,
  width: number,
  height: number,
) {
  return {
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
  };
}

test("end-align fits: right edge matches anchor right", () => {
  const anchor = rect(700, 40, 200, 32);
  const r = clampPopover({
    anchorRect: anchor,
    popoverSize: { width: 300, height: 200 },
    bounds: { left: 0, top: 0, right: 1000, bottom: 800 },
    preferAlign: "end",
    placement: "below",
  });
  assert.equal(r.left + 300, anchor.right);
  assert.equal(r.offsetLeft, r.left - anchor.left);
  assert.equal(r.placement, "below");
  assert.equal(r.top, anchor.bottom + 6);
});

test("shifts left when end-align would overflow bounds right", () => {
  const anchor = rect(880, 40, 120, 32);
  const r = clampPopover({
    anchorRect: anchor,
    popoverSize: { width: 500, height: 200 },
    bounds: { left: 0, top: 0, right: 1000, bottom: 800 },
    preferAlign: "end",
    placement: "below",
    pad: 8,
  });
  assert.ok(r.left + 500 <= 1000 - 8, `right=${r.left + 500}`);
  assert.ok(r.left >= 8, `left=${r.left}`);
});

test("respects content-pane bounds not full window", () => {
  const pane = { left: 280, top: 0, right: 1000, bottom: 800 };
  const anchor = rect(900, 40, 80, 32);
  const r = clampPopover({
    anchorRect: anchor,
    popoverSize: { width: 600, height: 200 },
    bounds: pane,
    preferAlign: "end",
    placement: "below",
    pad: 8,
  });
  assert.ok(r.left >= pane.left + 8);
  assert.ok(r.left + Math.min(600, pane.right - pane.left - 16) <= pane.right - 8);
});

test("auto placement opens above when below is tight", () => {
  const anchor = rect(100, 700, 160, 32);
  const r = clampPopover({
    anchorRect: anchor,
    popoverSize: { width: 200, height: 240 },
    bounds: { left: 0, top: 0, right: 1000, bottom: 780 },
    preferAlign: "start",
    placement: "auto",
    maxHeightCap: 260,
    gap: 6,
    pad: 8,
  });
  assert.equal(r.placement, "above");
  // 盒底应落在 trigger 上方（gap=6）
  assert.equal(r.top + Math.min(240, r.maxHeight), anchor.top - 6);
  assert.equal(r.offsetTop, r.top - anchor.top);
});

test("start-align flips to end when overflowing right", () => {
  const anchor = rect(850, 40, 100, 32);
  const r = clampPopover({
    anchorRect: anchor,
    popoverSize: { width: 280, height: 120 },
    bounds: { left: 0, top: 0, right: 1000, bottom: 800 },
    preferAlign: "start",
    placement: "below",
    pad: 8,
  });
  assert.ok(r.left + 280 <= 1000 - 8);
  // 翻转到 end 对齐附近
  assert.ok(Math.abs(r.left - (anchor.right - 280)) < 1 || r.left === 8);
});
