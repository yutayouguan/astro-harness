import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_SHELL_COLOR_PREFS,
  DEFAULT_SHELL_GRADIENT,
  effectiveUnifiedTone,
  hexToRgb,
  isNearBlack,
  isNearWhite,
  SHELL_GRADIENT_PRESETS,
  shellGradSpread,
  shellGradStrength,
  shellGradientPreviewBackground,
  shellHaloForRole,
  shellStopCountScale,
  underlayFromGradient,
  unifiedSurfaceMode,
} from "./shellGradient.ts";

test("defaults new installations to the dynamic color style", () => {
  assert.equal(DEFAULT_SHELL_COLOR_PREFS.style, "dynamic");
});

test("detects near white and near black", () => {
  assert.equal(isNearWhite("#ffffff"), true);
  assert.equal(isNearWhite("#2563eb"), false);
  assert.equal(isNearBlack("#0f172a"), true);
  assert.equal(isNearBlack("#000000"), true);
  assert.equal(isNearBlack("#94a3b8"), false);
});

test("unified surface mode for extreme picks", () => {
  assert.equal(unifiedSurfaceMode("light", "#ffffff"), "light-neutral");
  assert.equal(unifiedSurfaceMode("dark", "#0f172a"), "dark-neutral");
  assert.equal(unifiedSurfaceMode("light", "#2563eb"), "default");
});

test("effective unified tone falls back on light white / dark black", () => {
  assert.equal(effectiveUnifiedTone("light", "#ffffff"), "#64748b");
  assert.equal(effectiveUnifiedTone("dark", "#0f172a"), "#94a3b8");
  assert.equal(effectiveUnifiedTone("light", "#2563eb"), "#2563eb");
});

test("underlayFromGradient tints extreme colors", () => {
  const whiteGrad = {
    ...DEFAULT_SHELL_GRADIENT,
    primary: { ...DEFAULT_SHELL_GRADIENT.primary, color: "#ffffff" },
    secondary: { ...DEFAULT_SHELL_GRADIENT.secondary, color: "#ffffff" },
  };
  const blackGrad = {
    ...DEFAULT_SHELL_GRADIENT,
    primary: { ...DEFAULT_SHELL_GRADIENT.primary, color: "#0f172a" },
    secondary: { ...DEFAULT_SHELL_GRADIENT.secondary, color: "#0f172a" },
  };
  const lightUnder = underlayFromGradient("light", whiteGrad);
  const darkUnder = underlayFromGradient("dark", blackGrad);
  assert.notEqual(lightUnder, "#ffffff");
  assert.notEqual(darkUnder, "#0f172a");
});

test("stop count scales strength and spread automatically", () => {
  assert.equal(shellStopCountScale(2), 1.08);
  assert.equal(shellStopCountScale(5), 0.72);
  assert.ok(shellGradSpread(2) > shellGradSpread(5));

  const two = { ...DEFAULT_SHELL_GRADIENT, extras: [] };
  const five = {
    ...DEFAULT_SHELL_GRADIENT,
    extras: [
      { color: "#fb7185", x: 50, y: 50 },
      { color: "#22c55e", x: 35, y: 65 },
      { color: "#eab308", x: 68, y: 62 },
    ],
  };
  assert.ok(shellGradStrength("light", two) > shellGradStrength("light", five));
  assert.ok(
    shellHaloForRole("primary", "light").alpha >
      shellHaloForRole("extra", "light", 0).alpha,
  );
});

test("preview background shares formula and has no opaque solid blobs", () => {
  const css = shellGradientPreviewBackground(DEFAULT_SHELL_GRADIENT, "light");
  assert.match(css, /rgba\(/);
  assert.match(css, /transparent /);
  assert.doesNotMatch(css, /#3567c9 0%/);
  // 底部 50% 100% 有辅色回响，避免下半屏成为死区
  assert.match(css, /radial-gradient\(circle at 50% 100%, rgba\(79, 192, 184/);
});

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

test("presets keep both stops on opposite corners instead of one top band", () => {
  for (const preset of SHELL_GRADIENT_PRESETS) {
    const dx = Math.abs(preset.primary.x - preset.secondary.x);
    const dy = Math.abs(preset.primary.y - preset.secondary.y);
    assert.ok(
      Math.hypot(dx, dy) >= 50,
      `${preset.id} stops are too close (dx ${dx}, dy ${dy})`,
    );
    for (const value of [
      preset.primary.x,
      preset.primary.y,
      preset.secondary.x,
      preset.secondary.y,
    ]) {
      assert.ok(value >= 0 && value <= 100, `${preset.id} stop out of range`);
    }
  }
});

test("preset palette keeps real hue travel (indigo stays the quiet single-hue pair)", () => {
  const travelled = SHELL_GRADIENT_PRESETS.filter(
    (preset) => hueTravel(preset.primary.color, preset.secondary.color) >= 40,
  );
  assert.ok(
    travelled.length >= 6,
    `expected at least 6 presets with >= 40° hue travel, got ${travelled.length}`,
  );
  const colors = SHELL_GRADIENT_PRESETS.flatMap((preset) => [
    preset.primary.color,
    preset.secondary.color,
  ]);
  assert.equal(new Set(colors).size, colors.length, "duplicate preset colors");
});
