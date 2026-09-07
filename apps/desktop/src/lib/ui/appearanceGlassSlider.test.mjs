import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import {
  applyGlassIntensity,
  normalizeGlassIntensity,
  readStoredGlassIntensity,
} from "./glassIntensity.ts";

const [preferences, styles, theme, intensityRuntime, immersiveLight, main] =
  await Promise.all(
    [
      "../../components/settings/PreferencesPanel.tsx",
      "../../styles/features/preferences.css",
      "../../hooks/app/useTheme.tsx",
      "./glassIntensity.ts",
      "../../styles/tokens/immersive-light.css",
      "../../main.tsx",
    ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
  );

test("glass intensity uses one accessible continuous 0-100 range", () => {
  const control = preferences.slice(
    preferences.indexOf('id="appearance-glass-intensity"'),
    preferences.indexOf("prefs-card--appearance-color"),
  );

  assert.match(control, /type="range"/);
  assert.match(control, /min=\{GLASS_INTENSITY_MIN\}/);
  assert.match(control, /max=\{GLASS_INTENSITY_MAX\}/);
  assert.match(control, /step=\{1\}/);
  assert.match(control, /value=\{glassIntensity\}/);
  assert.match(control, /aria-valuetext=\{`\$\{glassIntensity\}%`\}/);
  assert.match(
    control,
    /setGlassIntensity\(Number\(event\.currentTarget\.value\)\)/,
  );
  assert.match(control, /\{glassIntensity\}%/);
  assert.doesNotMatch(control, /glassOptions|appearance-glass-slider-ticks/);
  assert.doesNotMatch(control, /role="radiogroup"/);
});

test("glass range stays contained and exposes progress, focus, and reduced motion", () => {
  assert.equal(
    preferences.match(/appearance-control-row appearance-control-row--split/g)
      ?.length,
    3,
  );
  assert.match(
    styles,
    /\.appearance-control-row--split\s*\{[\s\S]*?grid-template-columns:\s*minmax\(0, 0\.88fr\) minmax\(0, 1\.12fr\);/,
  );
  assert.match(
    styles,
    /\.appearance-control-row--split > :last-child\s*\{[\s\S]*?width:\s*100%;[\s\S]*?min-width:\s*0;[\s\S]*?max-width:\s*380px;[\s\S]*?box-sizing:\s*border-box;/,
  );
  assert.doesNotMatch(styles, /\.appearance-range-slider\s*\{[^}]*cqi/);
  assert.match(
    styles,
    /\.appearance-range-input::\-webkit-slider-runnable-track[\s\S]*?--appearance-range-progress/,
  );
  assert.match(
    styles,
    /\.appearance-range-input:focus-visible::\-webkit-slider-thumb/,
  );
  assert.match(
    styles,
    /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.appearance-range-input::\-webkit-slider-thumb[\s\S]*?transition:\s*none;/,
  );
});

test("theme state persists a normalized intensity with a balanced default of 64", () => {
  assert.match(intensityRuntime, /DEFAULT_GLASS_INTENSITY = 64/);
  assert.match(intensityRuntime, /GLASS_INTENSITY_MIN = 0/);
  assert.match(intensityRuntime, /GLASS_INTENSITY_MAX = 100/);
  assert.match(intensityRuntime, /"astro-glass-intensity"/);
  assert.match(intensityRuntime, /Math\.round\(numeric\)/);
  assert.match(
    intensityRuntime,
    /root\.dataset\.glassIntensity = String\(normalized\)/,
  );
  assert.match(
    intensityRuntime,
    /root\.style\.setProperty\("--glass-intensity", String\(normalized \/ 100\)\)/,
  );
  assert.doesNotMatch(theme, /GlassLevel|astro-glass-level|setGlassLevel/);
  assert.match(theme, /glassIntensity:\s*GlassIntensity/);
  assert.match(main, /readStoredGlassIntensity\(window\.localStorage\)/);
});

test("glass intensity normalization clamps input and rejects the removed presets", () => {
  assert.equal(readStoredGlassIntensity({ getItem: () => null }), 64);
  assert.equal(readStoredGlassIntensity({ getItem: () => "liquid" }), 64);
  assert.equal(normalizeGlassIntensity(-20), 0);
  assert.equal(normalizeGlassIntensity(42.6), 43);
  assert.equal(normalizeGlassIntensity(140), 100);

  const properties = new Map();
  const root = {
    dataset: {},
    removeAttribute(name) {
      assert.equal(name, "data-glass");
    },
    style: {
      setProperty(name, value) {
        properties.set(name, value);
      },
    },
  };
  applyGlassIntensity(root, 75);
  assert.equal(root.dataset.glassIntensity, "75");
  assert.equal(properties.get("--glass-intensity"), "0.75");
});

test("continuous immersive recipe drives color, blur, rim, and shadow", () => {
  for (const token of [
    "--immersive-glass-color",
    "--immersive-glass-background",
    "--immersive-glass-backdrop",
    "--immersive-glass-rim",
    "--immersive-glass-shadow",
  ]) {
    assert.match(immersiveLight, new RegExp(`${token}:`));
  }
  assert.match(immersiveLight, /var\(--glass-intensity, 0\.64\)/);
  assert.doesNotMatch(immersiveLight, /--liquid-glass-|--global-liquid-glass-/);
});

test("one immersive recipe drives every global surface family", () => {
  for (const alias of [
    "--glass-fill",
    "--sidebar-bg",
    "--composer-bg",
    "--menu-glass-bg",
    "--header-chip-bg",
  ]) {
    assert.match(
      immersiveLight,
      new RegExp(`${alias}: var\\(--immersive-glass-color\\)`),
      `${alias} must use the immersive glass color`,
    );
  }

  for (const alias of ["--glass-card", "--content-card-background"]) {
    assert.match(
      immersiveLight,
      new RegExp(`${alias}: var\\(--immersive-glass-background\\)`),
      `${alias} must use the immersive glass background`,
    );
  }

  for (const alias of [
    "--menu-overlay-bg",
    "--titlebar-menu-bg",
    "--lens-bg",
    "--badge-bg",
  ]) {
    assert.match(
      immersiveLight,
      new RegExp(`${alias}: var\\(--immersive-overlay-background\\)`),
      `${alias} must use the denser immersive overlay plane`,
    );
  }
});
