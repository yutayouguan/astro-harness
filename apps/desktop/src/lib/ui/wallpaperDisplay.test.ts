import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import {
  createWallpaperDisplaySaver,
  wallpaperDisplayValue,
  type WallpaperDisplay,
} from "./wallpaperDisplay.ts";

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
  assert.match(control, /window.addEventListener\("pointerup", finish\)/);
  assert.match(control, /event.type === "pointercancel"/);
  assert.doesNotMatch(control, /setPointerCapture/);
  assert.match(control, /disabled=\{!path \|\| \(disabled && !saving\)\}/);
  assert.match(control, /if \(!gesture.current\)/);
});

test("slow saves serialize gestures, coalesce queued values and never drop the final value", async () => {
  let current: WallpaperDisplay = { fit: "cover", shade: 18, blur: 0 };
  const calls: Partial<WallpaperDisplay>[] = [];
  const busy: boolean[] = [];
  let finish!: (ok: boolean) => void;
  const saver = createWallpaperDisplaySaver({
    current: () => current,
    commit: (patch) => {
      calls.push(patch);
      return calls.length === 1
        ? new Promise((resolve) => {
            finish = resolve;
          })
        : Promise.resolve(true);
    },
    applied: (patch) => {
      current = { ...current, ...patch };
    },
    rejected: () => assert.fail("unexpected failure"),
    busy: (value) => busy.push(value),
  });
  const done = saver.enqueue({ shade: 20 });
  void saver.enqueue({ shade: 30 });
  void saver.enqueue({ shade: 40, blur: 5 });
  assert.equal(saver.isSaving(), true);
  assert.equal(calls.length, 1);
  finish(true);
  await done;
  assert.deepEqual(calls, [{ shade: 20 }, { shade: 40, blur: 5 }]);
  assert.deepEqual(current, { fit: "cover", shade: 40, blur: 5 });
  assert.deepEqual(busy, [true, false]);
});

test("failed saves clear pending edits and finish busy state", async () => {
  let rejected = 0;
  const saver = createWallpaperDisplaySaver({
    current: () => ({ fit: "cover", shade: 18, blur: 0 }),
    commit: async () => {
      throw new Error("failure");
    },
    applied: () => assert.fail("not committed"),
    rejected: () => rejected++,
    busy: () => {},
  });
  await saver.enqueue({ shade: 20 });
  assert.equal(rejected, 1);
  assert.equal(saver.isSaving(), false);
});

test("closing a control cancels queued saves but lets the in-flight commit finish", async () => {
  let finish!: (ok: boolean) => void;
  let calls = 0;
  const saver = createWallpaperDisplaySaver({
    current: () => ({ fit: "cover", shade: 18, blur: 0 }),
    commit: () => {
      calls++;
      return new Promise((resolve) => {
        finish = resolve;
      });
    },
    applied: () => {},
    rejected: () => {},
    busy: () => {},
  });
  const done = saver.enqueue({ blur: 4 });
  void saver.enqueue({ blur: 8 });
  saver.clearPending();
  finish(true);
  await done;
  assert.equal(calls, 1);
});

test("display saves do not replay an unchanged global palette or reload scene lists", async () => {
  const source = await readFile(
    new URL("../../hooks/app/useDesktopAmbience.ts", import.meta.url),
    "utf8",
  );
  assert.match(
    source,
    /JSON.stringify\(colors\) !== JSON.stringify\(p.colors\)/,
  );
  assert.match(source, /if \(request.kind === "wallpaper_display"\) return/);
  assert.match(source, /\[open, sceneListKey\]/);
  assert.doesNotMatch(source, /\[open, pet.state.revision\]/);
});
