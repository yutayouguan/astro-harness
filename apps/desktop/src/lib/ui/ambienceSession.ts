import type { WallpaperPrefs } from "./wallpaper.ts";
import type { ShellColorPrefs } from "./shellGradient.ts";
import type { InterfaceMaterial } from "./interfaceMaterial.ts";
import type { ThemeMode } from "./themeResolution.ts";

export type AmbienceAppearance = {
  material: InterfaceMaterial;
  mode: ThemeMode;
  glassIntensity: number;
  softFrostIntensity: number;
};
export const appearanceKey = (value: AmbienceAppearance) =>
  JSON.stringify([
    value.material,
    value.mode,
    value.glassIntensity,
    value.softFrostIntensity,
  ]);

export type AmbienceUndoSnapshot = {
  kind: "native";
  token: string;
  wallpaper: WallpaperPrefs;
  colors: ShellColorPrefs;
  expected: string;
};
export type AppearanceUndoSnapshot = {
  kind: "appearance";
  before: AmbienceAppearance;
  expected: string;
};
/** Coalesce one gesture only while its last value is still current. */
export function appearanceEditUndo(
  current: AmbienceAppearance,
  next: AmbienceAppearance,
  gesture: AppearanceUndoSnapshot | null,
): AppearanceUndoSnapshot {
  return {
    kind: "appearance",
    before:
      gesture?.expected === appearanceKey(current) ? gesture.before : current,
    expected: appearanceKey(next),
  };
}
export type PetScaleUndoSnapshot = {
  kind: "pet-scale";
  petId: string;
  before: number;
  expected: number;
};
export const canUndoPetScale = (
  undo: PetScaleUndoSnapshot,
  current: { activePetId?: string | null; scale: number },
) => current.activePetId === undo.petId && current.scale === undo.expected;
type Undo =
  | AmbienceUndoSnapshot
  | AppearanceUndoSnapshot
  | PetScaleUndoSnapshot;
type Snapshot = { busy: boolean; undo: Undo | null };

/** WebView-session state, not persistent settings. Navigation must not discard native undo. */
export function createAmbienceSession() {
  let snapshot: Snapshot = { busy: false, undo: null };
  const listeners = new Set<() => void>();
  const publish = (next: Snapshot) => {
    snapshot = next;
    listeners.forEach((listener) => listener());
  };
  return {
    getSnapshot: () => snapshot,
    subscribe: (listener: () => void) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    begin: () => {
      if (snapshot.busy) return false;
      publish({ ...snapshot, busy: true });
      return true;
    },
    finish: () => publish({ ...snapshot, busy: false }),
    setUndo: (undo: Undo | null) => publish({ ...snapshot, undo }),
  };
}

// One main-window module instance. Reload/restart intentionally drops expired native tokens.
export const ambienceSession = createAmbienceSession();
