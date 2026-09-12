import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { wallpaperDisplayValue } from "./wallpaperDisplay.ts";

test("display controls share Appearance normalization and limits", () => {
  assert.deepEqual(
    wallpaperDisplayValue({ fit: "contain", shade: 18, blur: 0 }),
    { fit: "contain", shade: 18, blur: 0 },
  );
  assert.deepEqual(
    wallpaperDisplayValue({ fit: "stretch", shade: 100, blur: 99 }),
    { fit: "stretch", shade: 55, blur: 12 },
  );
  assert.deepEqual(wallpaperDisplayValue({ shade: -2, blur: -5 }), {
    fit: "cover",
    shade: 0,
    blur: 0,
  });
  assert.deepEqual(wallpaperDisplayValue({ shade: 18.4, blur: 2.7 }), {
    fit: "cover",
    shade: 18,
    blur: 3,
  });
});

test("wallpaper adjustments use a native display transaction, not a scene detach", async () => {
  const source = await readFile(
    new URL("../../hooks/app/useDesktopAmbience.ts", import.meta.url),
    "utf8",
  );
  const section = source.slice(
    source.indexOf("const setWallpaperDisplay"),
    source.indexOf("const stopFollowingSystem"),
  );
  assert.match(section, /current.current\?\.path !== path/);
  assert.match(section, /kind: "wallpaper_display"/);
  assert.match(section, /wallpaperDisplayValue/);
  assert.doesNotMatch(section, /setPalette|kind: "scene"|kind: "wallpaper"/);
});

test("display panel is in the wallpaper section and sliders commit at interaction boundaries", async () => {
  const panel = await readFile(
    new URL("../../components/ui/DesktopAmbienceButton.tsx", import.meta.url),
    "utf8",
  );
  const control = await readFile(
    new URL(
      "../../components/ui/AmbienceWallpaperDisplay.tsx",
      import.meta.url,
    ),
    "utf8",
  );
  assert.match(panel, /onCommit=\{state.setWallpaperDisplay\}/);
  assert.match(panel, /path=\{hasWallpaper \? current.current!.path : null\}/);
  assert.match(control, /<details className="ambience-wallpaper-display"/);
  assert.match(
    control,
    /onPointerUp=\{\(\) => void commit\(field, draftRef.current\[field\]\)\}/,
  );
  assert.match(
    control,
    /onPointerCancel=\{\(\) => update\(current.current\)\}/,
  );
  assert.match(control, /disabled=\{disabled \|\| saving \|\| !path\}/);
});
