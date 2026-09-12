import type { WallpaperPrefs } from "./wallpaper.ts";
import type { ShellColorPrefs } from "./shellGradient.ts";

export type AmbienceUndoSnapshot = {
  token: string;
  wallpaper: WallpaperPrefs;
  colors: ShellColorPrefs;
  expected: string;
};
type Snapshot = { busy: boolean; undo: AmbienceUndoSnapshot | null };

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
    setUndo: (undo: AmbienceUndoSnapshot | null) =>
      publish({ ...snapshot, undo }),
  };
}

// One main-window module instance. Reload/restart intentionally drops expired native tokens.
export const ambienceSession = createAmbienceSession();
