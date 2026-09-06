import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const app = await readFile(new URL("../../App.tsx", import.meta.url), "utf8");
const panel = await readFile(
  new URL(
    "../../components/settings/WallpaperSettingsCard.tsx",
    import.meta.url,
  ),
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
const unifiedStyles = await readFile(
  new URL("../../styles/tokens/unified-color.css", import.meta.url),
  "utf8",
);

test("wallpaper commands are registered and the settings card calls each source", () => {
  assert.match(commands, /commands::wallpaper::import_wallpaper/);
  assert.match(commands, /commands::wallpaper::generate_wallpaper/);
  assert.match(commands, /commands::wallpaper::analyze_wallpaper/);
  assert.match(commands, /commands::wallpaper::get_system_wallpaper/);
  assert.match(hook, /invoke<WallpaperAsset>\("import_wallpaper"/);
  assert.match(hook, /invoke<WallpaperAsset>\("generate_wallpaper"/);
  assert.match(hook, /invoke<WallpaperAnalysis>\("analyze_wallpaper"/);
  assert.match(hook, /invoke<WallpaperAsset>\("get_system_wallpaper"/);
  assert.match(panel, /controller\.importImage\(path\)/);
  assert.match(panel, /controller\.generate\(generatedPrompt\)/);
  assert.match(panel, /controller\.setFollowSystemWallpaper/);
});

test("app renders wallpaper behind shell chrome and fails closed on missing files", () => {
  assert.match(app, /className="shell-wallpaper-layer"/);
  assert.match(app, /onError=\{wallpaper\.markCurrentUnavailable\}/);
  assert.match(shellStyles, /\.shell-wallpaper-layer\s*\{/);
  assert.doesNotMatch(
    shellStyles,
    /\.app-shell\.has-wallpaper > :is\(\.native-drag-region, \.titlebar-sidebar-toggle, \.body-row\)/,
  );
  assert.doesNotMatch(shellStyles, /\.app-shell\.has-wallpaper > \.body-row/);
  assert.match(
    shellStyles,
    /\.native-drag-region\s*\{[\s\S]*?z-index:\s*var\(--z-window-drag\)/,
  );
  assert.match(
    shellStyles,
    /\.titlebar-sidebar-toggle\s*\{[\s\S]*?z-index:\s*var\(--z-window-controls\)/,
  );
});

test("wallpaper mode remains independent from color style", () => {
  assert.match(app, /const wallpaper = useWallpaper\(\)/);
  assert.match(app, /wallpaper=\{wallpaper\}/);
  assert.match(app, /colorStyle === "dynamic" \|\| wallpaperEnabled/);
  assert.match(hook, /cycleRecentWallpaper/);
  assert.match(app, /setWallpaperTheme\(recommendedWallpaperTheme\)/);
  assert.match(app, /applyWallpaperPaletteVars/);
  assert.match(app, /wallpaper\.prefs\.adaptiveColor/);
  assert.match(app, /data-wallpaper-palette/);
  assert.ok(
    unifiedStyles.lastIndexOf('html[data-wallpaper-palette="true"]') >
      unifiedStyles.lastIndexOf('[data-color-style="dynamic"]'),
  );
  assert.match(
    unifiedStyles,
    /--unified-tone:\s*var\(--wallpaper-tone, var\(--tone-blue\)\)/,
  );
});
