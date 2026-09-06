import assert from "node:assert/strict";
import { readFile, stat } from "node:fs/promises";
import test from "node:test";

const [
  provider,
  styles,
  index,
  morphIcon,
  navIcons,
  toolIcons,
  solidIcons,
  appIconHook,
  appIconOptions,
  preferences,
] = await Promise.all(
  [
    "../../hooks/app/useMorphicons.tsx",
    "../../styles/tokens/icon-preferences.css",
    "../../styles/index.css",
    "../../components/icons/MorphIcon.tsx",
    "../../components/icons/NavIcons.tsx",
    "../../components/icons/ToolIcons.tsx",
    "../../components/icons/GlassSolidIcons.tsx",
    "../../hooks/settings/useAppIcon.ts",
    "appIconOptions.ts",
    "../../components/settings/PreferencesPanel.tsx",
  ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
);

test("icon preferences publish one global motion and stroke contract", () => {
  assert.match(provider, /root\.dataset\.iconMotion = prefs\.spring/);
  assert.match(
    provider,
    /setProperty\([\s\S]*?"--app-icon-stroke-width",[\s\S]*?String\(prefs\.strokeWidth\),?[\s\S]*?\)/,
  );
  assert.match(index, /tokens\/icon-preferences\.css/);
  assert.match(styles, /--app-icon-motion-duration/);
  assert.match(styles, /--app-icon-stroke-width/);
  assert.match(styles, /svg\.lucide/);
  assert.match(styles, /prefers-reduced-motion: reduce/);
});

test("custom app icon families participate in the global icon contract", () => {
  assert.match(morphIcon, /app-ui-icon app-ui-icon--morph/);
  assert.match(navIcons, /app-ui-icon app-ui-icon--chrome/);
  assert.match(navIcons, /app-ui-icon app-ui-icon--nav/);
  assert.match(toolIcons, /app-ui-icon app-ui-icon--tool/);
  assert.match(solidIcons, /app-ui-icon app-ui-icon--solid/);
});

test("application icon choices have a complete frontend fallback", () => {
  for (const id of ["blue", "deep_blue", "black", "white", "white_logo"]) {
    assert.match(appIconOptions, new RegExp(`id: "${id}"`));
  }
  assert.match(appIconOptions, /remoteById\.get\(fallback\.id\) \?\? fallback/);
  assert.match(appIconHook, /appIconSettingsWithFallback\(null\)/);
  assert.match(appIconHook, /appIconSettingsWithFallback\(next, variant\)/);
  assert.match(preferences, /\(appIcon\?\.options \?\? \[\]\)\.map/);
});

test("application icon fallback thumbnails are bundled and lightweight", async () => {
  const sizes = await Promise.all(
    ["blue", "deep_blue", "black", "white", "white_logo"].map(
      async (id) =>
        (
          await stat(
            new URL(`../../assets/app-icons/${id}.png`, import.meta.url),
          )
        ).size,
    ),
  );
  for (const size of sizes) {
    assert.ok(size > 0);
    assert.ok(size < 100_000);
  }
});
