import { test } from "node:test";
import assert from "node:assert/strict";
import {
  createDynamicSeed,
  dynamicGradientForTab,
  hashString,
  hslToHex,
} from "./dynamicGradient.ts";
import { hexToRgb } from "./shellGradient.ts";

/** sRGB 色相角 0–360 */
function hueOf(hex: string): number {
  const rgb = hexToRgb(hex);
  assert.ok(rgb, `invalid hex ${hex}`);
  const [r, g, b] = [rgb.r, rgb.g, rgb.b].map((v) => v / 255);
  const max = Math.max(r, g, b);
  const d = max - Math.min(r, g, b);
  if (d === 0) return 0;
  let h: number;
  if (max === r) h = ((g - b) / d) % 6;
  else if (max === g) h = (b - r) / d + 2;
  else h = (r - g) / d + 4;
  return ((h * 60) + 360) % 360;
}

function hueTravel(a: string, b: string): number {
  const diff = Math.abs(hueOf(a) - hueOf(b));
  return Math.min(diff, 360 - diff);
}

const TABS = ["chat", "settings", "usage", "memory", "tools", "browser"];

test("dynamic gradient stays deterministic for a seed and tab", () => {
  const seed = "astro-default-seed";
  for (const tab of TABS) {
    const a = dynamicGradientForTab(seed, tab, "light");
    const b = dynamicGradientForTab(seed, tab, "light");
    assert.deepEqual(a, b);
  }
  const other = dynamicGradientForTab("another-seed", "chat", "light");
  const base = dynamicGradientForTab("astro-default-seed", "chat", "light");
  assert.notDeepEqual(other, base);
});

test("dynamic gradient keeps both stops on opposite corners", () => {
  for (const theme of ["light", "dark"] as const) {
    for (const tab of TABS) {
      const gradient = dynamicGradientForTab(
        `seed-${tab}-${theme}`,
        tab,
        theme,
      );
      const dx = gradient.secondary.x - gradient.primary.x;
      const dy = gradient.secondary.y - gradient.primary.y;
      assert.ok(
        dx > 0 && dy > 0,
        `${tab}/${theme} stops are not on a top-left → bottom-right diagonal`,
      );
      assert.ok(
        Math.hypot(dx, dy) >= 50,
        `${tab}/${theme} stops are too close (dx ${dx}, dy ${dy})`,
      );
    }
  }
});

test("dynamic gradient keeps real hue travel and readable stop colors", () => {
  for (const theme of ["light", "dark"] as const) {
    for (let i = 0; i < 40; i += 1) {
      const gradient = dynamicGradientForTab(
        createDynamicSeed(),
        `tab-${i}`,
        theme,
      );
      const travel = hueTravel(
        gradient.primary.color,
        gradient.secondary.color,
      );
      assert.ok(
        travel >= 35,
        `${theme} stop hues are only ${travel}° apart`,
      );
      for (const color of [gradient.primary.color, gradient.secondary.color]) {
        assert.match(color, /^#[0-9a-f]{6}$/);
      }
    }
  }
});

test("hslToHex wraps hue and clamps saturation and lightness", () => {
  assert.equal(hslToHex(0, 100, 50), hslToHex(360, 100, 50));
  assert.equal(hslToHex(-120, 100, 50), hslToHex(240, 100, 50));
  assert.equal(hslToHex(0, 200, 50), hslToHex(0, 100, 50));
  assert.equal(hslToHex(0, 50, 150), "#ffffff");
  assert.equal(hashString("") > 0, true);
});
