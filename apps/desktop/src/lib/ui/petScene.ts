import type { ActiveUiStyle } from "./activeUiStyle";
import type { DesktopPetState } from "./desktopPetState";
import type { WallpaperAsset } from "./wallpaper";
import type { PetPreferences } from "./petPreferences";

export type PetScene = {
  id: string;
  name: string;
  pet: Pick<
    DesktopPetState,
    | "sourcePath"
    | "groomingPath"
    | "motionClips"
    | "spriteVersionNumber"
    | "displayName"
    | "description"
    | "provider"
    | "model"
  > & { petPath: string; petId: string };
  style: ActiveUiStyle | null;
  wallpaperPath: string | null;
  inUse: boolean;
  favorite: boolean;
  preferences?: { scale: number; behavior: PetPreferences } | null;
};

export function groupPetScenes(scenes: PetScene[]) {
  const groups = new Map<
    string,
    { petPath: string; name: string | null; scenes: PetScene[] }
  >();
  for (const scene of scenes) {
    const group = groups.get(scene.pet.petId) ?? {
      petPath: scene.pet.petPath,
      name: scene.pet.displayName,
      scenes: [],
    };
    group.scenes.push(scene);
    groups.set(scene.pet.petId, group);
  }
  for (const group of groups.values())
    group.scenes.sort((a, b) => Number(b.favorite) - Number(a.favorite));
  return [...groups.values()];
}

export function sceneWallpaperAsset(
  style: ActiveUiStyle,
): WallpaperAsset | null {
  if (!style.wallpaper || !style.id.startsWith("pet-")) return null;
  return {
    id: style.id,
    name: style.name,
    path: style.wallpaper.path,
    source: "ai",
    width: 0,
    height: 0,
    createdAt: style.updatedAt,
    recommendedTheme: style.wallpaper.recommendedTheme,
    accentColor: style.wallpaper.accentColor,
    secondaryColor: style.wallpaper.secondaryColor,
  };
}

// One in-flight IPC; intermediate requests collapse to the newest wallpaper.
export function createWallpaperSync(
  sync: (path: string | null) => Promise<unknown>,
  error: (cause: unknown) => void,
) {
  let running = false;
  let pending: { path: string | null } | null = null;
  let disposed = false;
  let paused = false;
  let drained: (() => void)[] = [];
  const flush = async () => {
    if (running || paused) return;
    running = true;
    try {
      while (pending && !disposed) {
        const request = pending;
        pending = null;
        try {
          await sync(request.path);
        } catch (cause) {
          if (!disposed) error(cause);
        }
      }
    } finally {
      running = false;
      drained.splice(0).forEach((resolve) => resolve());
    }
  };
  return {
    request(path: string | null) {
      if (!disposed && !paused) {
        pending = { path };
        void flush();
      }
    },
    pause(): Promise<void> {
      paused = true;
      pending = null;
      return running
        ? new Promise((resolve) => drained.push(resolve))
        : Promise.resolve();
    },
    resume() {
      paused = false;
    },
    dispose() {
      disposed = true;
      pending = null;
    },
  };
}
