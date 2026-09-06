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
const preferencesCss = await readFile(
  new URL("../../styles/features/preferences.css", import.meta.url),
  "utf8",
);
const prototype = await readFile(
  new URL(
    "../../../../../designs/astro-wallpaper/settings-pages.jsx",
    import.meta.url,
  ),
  "utf8",
);
const prototypeCss = await readFile(
  new URL(
    "../../../../../designs/astro-wallpaper/settings-prototype.css",
    import.meta.url,
  ),
  "utf8",
);
const unifiedStyles = await readFile(
  new URL("../../styles/tokens/unified-color.css", import.meta.url),
  "utf8",
);
const welcomeStyles = await readFile(
  new URL("../../styles/features/chat/markdown.css", import.meta.url),
  "utf8",
);
const accessibilityStyles = await readFile(
  new URL("../../styles/tokens/a11y.css", import.meta.url),
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
  assert.match(panel, /className="wallpaper-preview-toggles"/);
  assert.match(panel, /prefs\.recent\.slice\(0, 2\)/);
  assert.match(panel, /className="wallpaper-recent-add"/);
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

test("image wallpaper editor stays hidden in ambient color mode", () => {
  assert.match(
    panel,
    /className="wallpaper-editor"[\s\S]{0,80}hidden=\{prefs\.mode !== "wallpaper"\}/,
  );
  assert.doesNotMatch(panel, /prefs\.mode === "color" \|\| prefs\.current/);
  assert.doesNotMatch(panel, /data-mode=\{mode\}/);
  assert.match(
    preferencesCss,
    /\.wallpaper-preview,[\s\S]*?\.wallpaper-empty-preview\s*\{[\s\S]*?height:\s*clamp\(220px, 28vw, 320px\);[\s\S]*?max-height:\s*320px;/,
  );
  assert.match(
    prototype,
    /settings\.backgroundMode === "wallpaper" \? <div className="wallpaper-editor">/,
  );
  assert.match(
    prototypeCss,
    /\.wallpaper-preview\s*\{[^}]*height:\s*clamp\(220px, 28vw, 320px\);[^}]*max-height:\s*320px;/,
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

test("wallpaper palette keeps chrome and welcome copy readable", () => {
  assert.match(
    unifiedStyles,
    /html\[data-theme="light"\]\[data-wallpaper="true"\][\s\S]*?--color-text:\s*#1d2732;[\s\S]*?--sidebar-bg:\s*rgba\(248, 250, 252, 0\.72\);/,
  );
  assert.match(
    unifiedStyles,
    /html\[data-theme="dark"\]\[data-wallpaper="true"\][\s\S]*?--color-text:\s*#f5f7fa;[\s\S]*?--sidebar-bg:\s*rgba\(8, 12, 20, 0\.7\);/,
  );
  assert.match(
    unifiedStyles,
    /--wallpaper-readable-tone:\s*color-mix\([\s\S]*?var\(--wallpaper-tone/,
  );
  assert.match(
    welcomeStyles,
    /html\[data-wallpaper="true"\] \.chat-welcome-copy::before\s*\{[\s\S]*?--wallpaper-content-scrim[\s\S]*?backdrop-filter:/,
  );
  assert.match(
    welcomeStyles,
    /html\[data-wallpaper="true"\] \.chat-welcome-title-brand\s*\{[\s\S]*?--wallpaper-readable-tone/,
  );
  assert.match(
    welcomeStyles,
    /html\[data-wallpaper="true"\] \.chat-welcome-sub\s*\{[\s\S]*?color:\s*var\(--ink-soft\);[\s\S]*?font-weight:\s*500;/,
  );
  assert.match(
    accessibilityStyles,
    /@media \(prefers-reduced-transparency: reduce\)[\s\S]*?\.chat-welcome-copy::before\s*\{[\s\S]*?backdrop-filter:\s*none !important;/,
  );
});
