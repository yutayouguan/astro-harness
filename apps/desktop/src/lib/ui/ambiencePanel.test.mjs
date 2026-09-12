import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
const read = (path) => readFile(new URL(path, import.meta.url), "utf8");
const [panel, hook, css] = await Promise.all([
  read("../../components/ui/DesktopAmbienceButton.tsx"),
  read("../../hooks/app/useDesktopAmbience.ts"),
  read("../../styles/features/desktop-ambience.css"),
]);
test("three contextual panels replace the independent shuffle selector", () => {
  for (const tab of ["material", "background", "pets"])
    assert.ok(panel.includes('value: "' + tab + '"'));
  assert.doesNotMatch(panel, /<select|换一个的范围|value: "palette"/);
  assert.match(panel, /state\.shuffle\(scope\)/);
  assert.match(panel, /selectedPet === state\.pet\.activePetId/);
  assert.match(panel, /animate=\{false\}/);
  assert.match(panel, /maxHeightRatio=\{0\.75\}/);
  assert.match(css, /\.ambience-content\s*\{[^}]*overflow: auto/);
});
test("material edits reuse theme state and gesture boundaries; native undo stays separate", () => {
  assert.match(hook, /const theme = useTheme\(\)/);
  for (const setter of [
    "setMaterial",
    "setMode",
    "setGlassIntensity",
    "setSoftFrostIntensity",
  ])
    assert.ok(hook.includes("theme." + setter));
  assert.match(panel, /onPointerDown=\{state.beginAppearanceEdit\}/);
  assert.match(panel, /onBlur=\{state.finishAppearanceEdit\}/);
  const branch = hook.slice(
    hook.indexOf('if (undo.kind === "appearance")'),
    hook.indexOf("const saveAs"),
  );
  assert.match(
    branch,
    /appearanceKey\(appearanceLive.current\) !== undo.expected/,
  );
  assert.match(branch, /applyAppearance\(undo.before\)[\s\S]*?return;/);
  assert.match(branch, /undo_desktop_ambience/);
});
test("pet browsing is local; only explicit scene picks call the native switch", () => {
  assert.match(panel, /onClick=\{\(\) => setBrowsedPet\(petId\)\}/);
  assert.match(panel, /state.selectScene\(item.id\)/);
  assert.match(panel, /<DesktopPetVisibilityButton/);
  assert.match(panel, /reducedMotion/);
});
test("color mode only commits its visual selection after a successful change", () => {
  assert.match(
    panel,
    /state.clearWallpaper\(\).then\(\(ok\) => \{\s*if \(ok\) setBackground\("color"\)/,
  );
  assert.match(panel, /\[hasWallpaper, open\]/);
  assert.match(hook, /setAmbientColors[\s\S]*?kind: "wallpaper", path: null/);
  assert.match(hook, /stopFollowingSystem[\s\S]*?followSystemWallpaper: false/);
});
