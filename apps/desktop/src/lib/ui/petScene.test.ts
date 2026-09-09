import assert from "node:assert/strict";
import test from "node:test";
import {
  createWallpaperSync,
  sceneWallpaperAsset,
  groupPetScenes,
  type PetScene,
} from "./petScene.ts";
import type { ActiveUiStyle } from "./activeUiStyle.ts";

const turn = () => new Promise<void>((resolve) => setImmediate(resolve));

test("pet groups retain identity across homes and sort favorites without mutating input", () => {
  const pet = {
    groomingPath: null,
    petPath: "/managed/pet.png",
    sourcePath: null,
    spriteVersionNumber: null,
    displayName: "Mochi",
    description: null,
    provider: null,
    model: null,
  };
  const a: PetScene = {
    id: "a",
    name: "Forest",
    pet,
    style: null,
    wallpaperPath: null,
    inUse: false,
    favorite: false,
  };
  const b = { ...a, id: "b", name: "Beach", favorite: true };
  const c = { ...a, id: "c", pet: { ...pet, petPath: "/managed/other.png" } };
  const scenes = [a, b, c];
  const groups = groupPetScenes(scenes);
  assert.equal(groups.length, 2);
  assert.deepEqual(
    groups[0].scenes.map((s) => s.id),
    ["b", "a"],
  );
  assert.deepEqual(
    scenes.map((s) => s.id),
    ["a", "b", "c"],
  );
});

test("scene wallpaper switching is serialized and intermediate picks collapse", async () => {
  let release!: () => void;
  const first = new Promise<void>((resolve) => {
    release = resolve;
  });
  const seen: Array<string | null> = [];
  const sync = createWallpaperSync(async (path) => {
    seen.push(path);
    if (seen.length === 1) await first;
  }, assert.fail);
  sync.request("a");
  sync.request("b");
  sync.request("c");
  assert.deepEqual(seen, ["a"]);
  release();
  await turn();
  assert.deepEqual(seen, ["a", "c"]);
  sync.dispose();
});

test("wallpaper sync failures do not block the newest selection", async () => {
  const seen: Array<string | null> = [];
  const errors: unknown[] = [];
  const sync = createWallpaperSync(
    async (path) => {
      seen.push(path);
      if (path === "bad") throw new Error("missing");
    },
    (error) => errors.push(error),
  );
  sync.request("bad");
  sync.request(null);
  await turn();
  assert.deepEqual(seen, ["bad", null]);
  assert.equal(errors.length, 1);
  sync.dispose();
});

test("disposing scene sync prevents queued writes", async () => {
  const seen: Array<string | null> = [];
  const sync = createWallpaperSync(async (path) => {
    seen.push(path);
  }, assert.fail);
  sync.request("a");
  sync.request("b");
  sync.dispose();
  await turn();
  sync.request("c");
  assert.deepEqual(seen, ["a"]);
});

test("only pet scene styles enter the wallpaper recent library", () => {
  const style: ActiveUiStyle = {
    schemaVersion: 1,
    id: "pet-forest",
    name: "Forest",
    revision: "r1",
    updatedAt: "now",
    tokens: { light: {}, dark: {} },
    icons: {},
    wallpaper: {
      path: "/managed/wallpaper.png",
      fit: "cover",
      shade: 18,
      blur: 0,
      adaptiveColor: true,
    },
  };
  assert.equal(sceneWallpaperAsset(style)?.path, "/managed/wallpaper.png");
  assert.equal(sceneWallpaperAsset({ ...style, id: "ordinary-theme" }), null);
  assert.equal(sceneWallpaperAsset({ ...style, wallpaper: undefined }), null);
});
