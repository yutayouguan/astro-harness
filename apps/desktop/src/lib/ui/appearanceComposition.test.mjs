import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const read = (path) => readFile(new URL(path, import.meta.url), "utf8");
const [preferences, wallpaper, css] = await Promise.all([
  read("../../components/settings/PreferencesPanel.tsx"),
  read("../../components/settings/WallpaperSettingsCard.tsx"),
  read("../../styles/features/preferences.css"),
]);

test("motion occupies the second appearance card, with colors inside the background card", () => {
  const material = preferences.indexOf(
    'className="prefs-card prefs-card--appearance-material"',
  );
  const motion = preferences.indexOf(
    'className="prefs-card prefs-card--appearance-motion morphicon-settings-card"',
  );
  const background = preferences.indexOf("<WallpaperSettingsCard");
  assert.ok(material >= 0 && material < motion && motion < background);
  assert.doesNotMatch(preferences + css, /prefs-card--appearance-color/);
  assert.equal(
    preferences.match(/prefs\.appearance\.motion\.title/g)?.length,
    1,
  );
  const colors = preferences.slice(
    background,
    preferences.indexOf('className="prefs-card prefs-card--app-icon"'),
  );
  assert.match(colors, /colorControls=\{/);
  for (const callback of [
    "onColorStyleChange(id)",
    "onGradientChange(g)",
    "onBeginCustomGradient()",
    "onClick={onReshuffleDynamic}",
  ])
    assert.ok(colors.includes(callback), callback);
});

test("background modes exclusively expose their controls without discarding their state", () => {
  assert.match(
    wallpaper,
    /className="wallpaper-color-editor"\s+hidden=\{prefs.mode !== "color"\}\s*>\s*\{colorControls\}/,
  );
  assert.match(
    wallpaper,
    /className="wallpaper-editor"\s+hidden=\{prefs.mode !== "wallpaper"\}/,
  );
  assert.match(
    css,
    /\.wallpaper-color-editor\[hidden\],\s*\.wallpaper-editor\[hidden\]\s*\{\s*display: none;/,
  );
  assert.match(
    css,
    /\.prefs-card--appearance-motion \.morphicon-setting-row\s*\{\s*grid-template-columns: minmax\(0, 1fr\)/,
  );
  assert.match(
    css,
    /@container \(max-width: 760px\)[\s\S]*\.prefs-card--appearance-motion\s*\{\s*grid-column: 1;/,
  );
});
