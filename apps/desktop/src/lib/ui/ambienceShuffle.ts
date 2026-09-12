import { normalizeWallpaperAsset, type WallpaperAsset } from "./wallpaper.ts";
import type { PetScene } from "./petScene.ts";

export const AMBIENCE_SHUFFLE_KEY = "astro-ambience-shuffle.v1";
export const MAX_FAVORITE_WALLPAPERS = 100;
export type ShuffleScope = "pet-scenes" | "favorites" | "palette";
export type ShufflePrefs = {
  scope: ShuffleScope;
  backgroundLocked: boolean;
  favorites: WallpaperAsset[];
};
export const DEFAULT_SHUFFLE_PREFS: ShufflePrefs = {
  scope: "pet-scenes",
  backgroundLocked: false,
  favorites: [],
};

export function normalizeShufflePrefs(raw: unknown): ShufflePrefs {
  const value =
    raw && typeof raw === "object" ? (raw as Partial<ShufflePrefs>) : {};
  const unique = new Map<string, WallpaperAsset>();
  if (Array.isArray(value.favorites)) {
    for (const item of value.favorites) {
      const asset = normalizeWallpaperAsset(item);
      if (
        asset &&
        !unique.has(asset.path) &&
        unique.size < MAX_FAVORITE_WALLPAPERS
      )
        unique.set(asset.path, asset);
    }
  }
  return {
    scope:
      value.scope === "favorites" || value.scope === "palette"
        ? value.scope
        : "pet-scenes",
    backgroundLocked: value.backgroundLocked === true,
    favorites: [...unique.values()],
  };
}

export function toggleFavoriteWallpaper(
  prefs: ShufflePrefs,
  asset: WallpaperAsset,
): ShufflePrefs {
  const existing = prefs.favorites.some((item) => item.path === asset.path);
  if (!existing && prefs.favorites.length >= MAX_FAVORITE_WALLPAPERS)
    throw new Error("最多收藏100张壁纸，请先取消部分收藏");
  return normalizeShufflePrefs({
    ...prefs,
    favorites: existing
      ? prefs.favorites.filter((item) => item.path !== asset.path)
      : [...prefs.favorites, asset],
  });
}

export type ShuffleChoice =
  | { kind: "scene"; sceneId: string; expectedPetId: string }
  | { kind: "wallpaper"; asset: WallpaperAsset }
  | { kind: "palette" };
export type ShuffleAvailability = {
  choices: ShuffleChoice[];
  reason: "locked" | "no-pet" | "no-scene" | "no-favorite" | null;
};

export function shuffleAvailability(
  prefs: ShufflePrefs,
  scenes: PetScene[],
  petId: string | null | undefined,
  currentSceneId: string | null | undefined,
  currentPath: string | null,
): ShuffleAvailability {
  if (prefs.scope === "palette")
    return { choices: [{ kind: "palette" }], reason: null };
  if (prefs.backgroundLocked) return { choices: [], reason: "locked" };
  if (prefs.scope === "favorites") {
    const choices: ShuffleChoice[] = prefs.favorites
      .filter((asset) => asset.path !== currentPath)
      .map((asset) => ({ kind: "wallpaper", asset }));
    return { choices, reason: choices.length ? null : "no-favorite" };
  }
  if (!petId) return { choices: [], reason: "no-pet" };
  const unique = new Map<string, ShuffleChoice>();
  for (const scene of scenes) {
    if (
      scene.pet.petId === petId &&
      scene.id !== currentSceneId &&
      scene.wallpaperPath &&
      scene.wallpaperPath !== currentPath &&
      !unique.has(scene.wallpaperPath)
    ) {
      unique.set(scene.wallpaperPath, {
        kind: "scene",
        sceneId: scene.id,
        expectedPetId: petId,
      });
    }
  }
  const choices = [...unique.values()];
  return { choices, reason: choices.length ? null : "no-scene" };
}

export function chooseShuffleCandidate(
  choices: ShuffleChoice[],
  random = Math.random,
): ShuffleChoice | null {
  if (!choices.length) return null;
  const sample = random();
  return (
    choices[
      Math.min(
        choices.length - 1,
        Math.max(
          0,
          Math.floor((Number.isFinite(sample) ? sample : 0) * choices.length),
        ),
      )
    ] ?? null
  );
}
