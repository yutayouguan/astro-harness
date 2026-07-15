import { test } from "node:test";
import assert from "node:assert/strict";
import { clampModelPickerFlyout } from "./modelPickerFlyout.ts";

test("default right-aligns when flyout fits", () => {
  // trigger 右缘在 900；bounds 0..1000；flyout 300 → left = 200-300 = -100 relative
  const pos = clampModelPickerFlyout(
    { left: 700, width: 200 },
    300,
    { left: 0, right: 1000 },
    8,
  );
  assert.equal(pos.right, "auto");
  assert.equal(pos.left, 200 - 300);
  assert.equal(700 + pos.left + 300, 900);
});

test("shifts left when edit panel would overflow bounds right", () => {
  // trigger 贴右：left=880, width=120 → right edge 1000
  // flyout 500（列表+编辑）右对齐会超出 1000-8
  const pos = clampModelPickerFlyout(
    { left: 880, width: 120 },
    500,
    { left: 0, right: 1000 },
    8,
  );
  const absRight = 880 + pos.left + 500;
  assert.ok(absRight <= 1000 - 8, `absRight=${absRight}`);
  assert.ok(880 + pos.left >= 8, `absLeft=${880 + pos.left}`);
  // 必须比纯右对齐更靠左
  assert.ok(pos.left < 120 - 500);
});

test("clamps within content-pane bounds not full window", () => {
  // 侧栏占左 280；content-pane 280..1000；trigger 在 pane 右侧
  const pane = { left: 280, right: 1000 };
  const pos = clampModelPickerFlyout(
    { left: 900, width: 80 },
    600,
    pane,
    8,
  );
  assert.ok(900 + pos.left >= pane.left + 8);
  assert.ok(900 + pos.left + Math.min(600, pane.right - pane.left - 16) <= pane.right - 8);
});
