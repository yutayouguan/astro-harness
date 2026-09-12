import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_SHUFFLE_PREFS,
  normalizeShufflePrefs,
  toggleFavoriteWallpaper,
  shuffleAvailability,
  chooseShuffleCandidate,
} from "./ambienceShuffle.ts";
import type { PetScene } from "./petScene.ts";
import type { WallpaperAsset } from "./wallpaper.ts";
const asset = (n: number): WallpaperAsset => ({
  id: `${n}`,
  path: `/ui/${n}.png`,
  name: `Image ${n}`,
  source: "upload",
  width: 10,
  height: 10,
  createdAt: "now",
});
const scene = (id: string, petId: string, path: string | null): PetScene => ({
  id,
  name: id,
  pet: {
    petId,
    petPath: `/ui/${petId}.png`,
    sourcePath: null,
    groomingPath: null,
    spriteVersionNumber: null,
    displayName: petId,
    description: null,
    provider: null,
    model: null,
  },
  wallpaperPath: path,
  style: null,
  favorite: false,
  inUse: false,
});
const scenes = [
  scene("a", "cat", "/ui/1.png"),
  scene("b", "cat", "/ui/2.png"),
  scene("c", "dog", "/ui/3.png"),
  scene("no-image", "cat", null),
  scene("duplicate", "cat", "/ui/2.png"),
];

test("current pet scene pool excludes other pets, current wallpaper, missing and duplicate images", () => {
  const result = shuffleAvailability(
    DEFAULT_SHUFFLE_PREFS,
    scenes,
    "cat",
    "a",
    "/ui/1.png",
  );
  assert.deepEqual(result.choices, [
    { kind: "scene", sceneId: "b", expectedPetId: "cat" },
  ]);
  assert.equal(
    shuffleAvailability(DEFAULT_SHUFFLE_PREFS, scenes, null, null, null).reason,
    "no-pet",
  );
  assert.equal(
    shuffleAvailability(DEFAULT_SHUFFLE_PREFS, scenes, "dog", "c", "/ui/3.png")
      .reason,
    "no-scene",
  );
});
test("favorites shuffle only changes wallpaper and excludes the current path", () => {
  const prefs = normalizeShufflePrefs({
    scope: "favorites",
    favorites: [asset(1), asset(3)],
  });
  assert.deepEqual(
    shuffleAvailability(prefs, scenes, "cat", "a", "/ui/1.png").choices,
    [{ kind: "wallpaper", asset: prefs.favorites[1] }],
  );
  assert.equal(
    shuffleAvailability(
      { ...prefs, favorites: [asset(1)] },
      scenes,
      "cat",
      "a",
      "/ui/1.png",
    ).reason,
    "no-favorite",
  );
});
test("background lock blocks both background scopes but not colors", () => {
  for (const scope of ["pet-scenes", "favorites"] as const)
    assert.equal(
      shuffleAvailability(
        {
          ...DEFAULT_SHUFFLE_PREFS,
          scope,
          backgroundLocked: true,
          favorites: [asset(2)],
        },
        scenes,
        "cat",
        "a",
        "/ui/1.png",
      ).reason,
      "locked",
    );
  assert.deepEqual(
    shuffleAvailability(
      { ...DEFAULT_SHUFFLE_PREFS, scope: "palette", backgroundLocked: true },
      [],
      null,
      null,
      null,
    ).choices,
    [{ kind: "palette" }],
  );
});
test("normalization is bounded, deduplicates by file, rejects malformed assets, and roundtrips", () => {
  const prefs = normalizeShufflePrefs({
    scope: "invalid",
    backgroundLocked: "yes",
    favorites: [
      null,
      {},
      asset(1),
      { ...asset(1), id: "alias" },
      ...Array.from({ length: 110 }, (_, i) => asset(i + 2)),
    ],
  });
  assert.equal(prefs.scope, "pet-scenes");
  assert.equal(prefs.backgroundLocked, false);
  assert.equal(prefs.favorites.length, 100);
  assert.deepEqual(
    normalizeShufflePrefs(JSON.parse(JSON.stringify(prefs))),
    prefs,
  );
  assert.throws(() => toggleFavoriteWallpaper(prefs, asset(999)), /100/);
  assert.equal(toggleFavoriteWallpaper(prefs, asset(1)).favorites.length, 99);
});
test("favorites do not depend on recent history and removing a star does not mutate the original", () => {
  const saved = toggleFavoriteWallpaper(DEFAULT_SHUFFLE_PREFS, asset(9));
  assert.equal(saved.favorites[0]?.path, "/ui/9.png");
  assert.equal(DEFAULT_SHUFFLE_PREFS.favorites.length, 0);
  assert.equal(
    toggleFavoriteWallpaper(saved, { ...asset(9), id: "new-id" }).favorites
      .length,
    0,
  );
});
test("empty pools never invoke random or invent generation; random picks only known candidates", () => {
  assert.equal(
    chooseShuffleCandidate([], () => {
      throw new Error("must not call");
    }),
    null,
  );
  const choices = shuffleAvailability(
    DEFAULT_SHUFFLE_PREFS,
    scenes,
    "cat",
    "a",
    "/ui/1.png",
  ).choices;
  for (const number of [0, 0.999, 1, -1, NaN])
    assert.equal(
      chooseShuffleCandidate(choices, () => number),
      choices[0],
    );
});
