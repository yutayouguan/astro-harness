import { normalizeWallpaperPrefs, type WallpaperPrefs } from "./wallpaper.ts";

export type WallpaperDisplay = Pick<WallpaperPrefs, "fit" | "shade" | "blur">;

/** Serialize completed gestures and keep only the newest queued field values. */
export function createWallpaperDisplaySaver(options: {
  current: () => WallpaperDisplay;
  commit: (patch: Partial<WallpaperDisplay>) => Promise<boolean>;
  applied: (patch: Partial<WallpaperDisplay>) => void;
  rejected: () => void;
  busy: (value: boolean) => void;
}) {
  let pending = false;
  let queued: Partial<WallpaperDisplay> | null = null;
  let completion = Promise.resolve();
  async function drain() {
    pending = true;
    options.busy(true);
    try {
      while (queued) {
        const patch = queued;
        queued = null;
        if (
          Object.entries(patch).every(
            ([key, value]) =>
              options.current()[key as keyof WallpaperDisplay] === value,
          )
        )
          continue;
        if (!(await options.commit(patch))) {
          queued = null;
          options.rejected();
          break;
        }
        options.applied(patch);
      }
    } catch {
      queued = null;
      options.rejected();
    } finally {
      pending = false;
      options.busy(false);
    }
  }
  return {
    isSaving: () => pending,
    clearPending: () => {
      queued = null;
    },
    enqueue: (patch: Partial<WallpaperDisplay>) => {
      queued = { ...queued, ...patch };
      if (!pending) completion = drain();
      return completion;
    },
  };
}

/** Use exactly the same fit/shade/blur normalization as Appearance settings. */
export function wallpaperDisplayValue(
  value: Partial<WallpaperDisplay>,
): WallpaperDisplay {
  const { fit, shade, blur } = normalizeWallpaperPrefs(value);
  return { fit, shade, blur };
}
