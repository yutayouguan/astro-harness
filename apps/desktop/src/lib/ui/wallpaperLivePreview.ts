import {
  wallpaperDisplayValue,
  type WallpaperDisplay,
} from "./wallpaperDisplay.ts";

type PreviewValues = Partial<Pick<WallpaperDisplay, "shade" | "blur">>;
export type WallpaperPreviewTarget = {
  style: Pick<
    CSSStyleDeclaration,
    "getPropertyValue" | "setProperty" | "removeProperty"
  >;
  matches: () => boolean;
  observe: (changed: () => void) => () => void;
};

/** Preview only the image layer. Keep the preview until committed CSS catches up. */
export function createWallpaperLivePreview(
  resolve: (path: string) => WallpaperPreviewTarget | null,
) {
  let active: WallpaperPreviewTarget | null = null;
  let activePath: string | null = null;
  let values: PreviewValues = {};
  let stop: (() => void) | null = null;
  let settling = false;
  function cancel() {
    stop?.();
    stop = null;
    const target = active;
    active = null;
    activePath = null;
    values = {};
    settling = false;
    target?.style.removeProperty("--wallpaper-live-shade");
    target?.style.removeProperty("--wallpaper-live-blur");
  }
  function check() {
    if (!active) return;
    if (!active.matches()) {
      cancel();
      return;
    }
    if (!settling) return;
    const confirmed = Object.entries(values).every(([key, value]) => {
      const base = Number.parseFloat(
        active!.style.getPropertyValue("--wallpaper-" + key),
      );
      return (
        Math.abs(base - (key === "shade" ? value / 100 : value)) < 0.000001
      );
    });
    if (confirmed) cancel();
  }
  return {
    show(path: string, patch: PreviewValues) {
      if (!active || activePath !== path || !active.matches()) {
        cancel();
        active = resolve(path);
        activePath = path;
        if (!active) return;
        stop = active.observe(check);
      }
      settling = false;
      const normalized = wallpaperDisplayValue(patch);
      if (patch.shade !== undefined) {
        values.shade = normalized.shade;
        active.style.setProperty(
          "--wallpaper-live-shade",
          String(normalized.shade / 100),
        );
      }
      if (patch.blur !== undefined) {
        values.blur = normalized.blur;
        active.style.setProperty(
          "--wallpaper-live-blur",
          String(normalized.blur),
        );
      }
    },
    settle() {
      settling = true;
      check();
    },
    cancel,
  };
}
