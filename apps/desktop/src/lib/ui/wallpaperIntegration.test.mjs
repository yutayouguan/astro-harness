import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const app = await readFile(new URL("../../App.tsx", import.meta.url), "utf8");
const panel = await readFile(
  new URL("../../components/settings/WallpaperSettingsCard.tsx", import.meta.url),
  "utf8",
);
const hook = await readFile(
  new URL("../../hooks/app/useWallpaper.ts", import.meta.url),
  "utf8",
);
const commands = await readFile(
  new URL("../../../src-tauri/src/lib.rs", import.meta.url),
  "utf8",
);
const shellStyles = await readFile(
  new URL("../../styles/features/shell/shell.css", import.meta.url),
  "utf8",
);

test("wallpaper commands are registered and the settings card calls both paths", () => {
  assert.match(commands, /commands::wallpaper::import_wallpaper/);
  assert.match(commands, /commands::wallpaper::generate_wallpaper/);
  assert.match(hook, /invoke<WallpaperAsset>\("import_wallpaper"/);
  assert.match(hook, /invoke<WallpaperAsset>\("generate_wallpaper"/);
  assert.match(panel, /controller\.importImage\(path\)/);
  assert.match(panel, /controller\.generate\(generatedPrompt\)/);
});

test("app renders wallpaper behind shell chrome and fails closed on missing files", () => {
  assert.match(app, /className="shell-wallpaper-layer"/);
  assert.match(app, /onError=\{wallpaper\.markCurrentUnavailable\}/);
  assert.match(shellStyles, /\.shell-wallpaper-layer\s*\{/);
  assert.match(shellStyles, /\.app-shell\.has-wallpaper > \.body-row/);
});

test("wallpaper mode remains independent from color style", () => {
  assert.match(app, /const wallpaper = useWallpaper\(\)/);
  assert.match(app, /wallpaper=\{wallpaper\}/);
  assert.match(app, /colorStyle === "dynamic" && !wallpaperEnabled/);
});
