export const DESKTOP_PET_SCALE = {
  min: 0.3,
  max: 0.6,
  step: 0.01,
  reference: 0.4,
} as const;

export const petScalePercent = (scale: number) =>
  Math.round((scale / DESKTOP_PET_SCALE.reference) * 1000) / 10;
export const petScaleFromPercent = (percent: number) =>
  Number.isFinite(percent)
    ? Math.max(
        DESKTOP_PET_SCALE.min,
        Math.min(
          DESKTOP_PET_SCALE.max,
          (percent * DESKTOP_PET_SCALE.reference) / 100,
        ),
      )
    : DESKTOP_PET_SCALE.reference;

import type { PetMotionClips } from "./petMotionClip";
import {
  DEFAULT_PET_PREFERENCES,
  type PetPreferences,
} from "./petPreferences.ts";

export type DesktopPetState = {
  revision: number;
  enabled: boolean;
  sourcePath: string | null;
  petPath: string | null;
  groomingPath: string | null;
  motionClips?: PetMotionClips;
  scale: number;
  preferences?: PetPreferences;
  alwaysOnTop: boolean;
  updatedAt: string;
  provider: string | null;
  model: string | null;
  spriteVersionNumber: number | null;
  displayName: string | null;
  description: string | null;
  followWallpaper: boolean;
  lastWallpaperPath: string | null;
  scenes: Array<{ id: string; name: string }>;
  favoriteSceneIds: string[];
  animationPaused: boolean;
};

export const EMPTY_DESKTOP_PET_STATE: DesktopPetState = {
  revision: 0,
  enabled: false,
  sourcePath: null,
  petPath: null,
  groomingPath: null,
  motionClips: {},
  scale: DESKTOP_PET_SCALE.reference,
  preferences: DEFAULT_PET_PREFERENCES,
  alwaysOnTop: true,
  updatedAt: "",
  provider: null,
  model: null,
  spriteVersionNumber: null,
  displayName: null,
  description: null,
  followWallpaper: false,
  lastWallpaperPath: null,
  scenes: [],
  favoriteSceneIds: [],
  animationPaused: false,
};

export function acceptDesktopPetState(
  current: DesktopPetState,
  next: DesktopPetState,
) {
  if (!Number.isSafeInteger(next.revision) || next.revision < current.revision)
    return current;
  return next;
}

export function createPetMutationQueue() {
  let tail = Promise.resolve();
  return <T>(operation: () => Promise<T>): Promise<T> => {
    const result = tail.then(operation);
    tail = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  };
}

// Subscribe first; revisions resolve snapshot/live interleaving. Dispose is synchronous,
// even when the native listener registration itself has not resolved yet.
export function subscribeDesktopPetState(options: {
  listen: (receive: (state: DesktopPetState) => void) => Promise<() => void>;
  snapshot: () => Promise<DesktopPetState>;
  receive: (state: DesktopPetState) => void;
  error: (error: unknown) => void;
  ready: () => void;
}) {
  let disposed = false;
  let stop: (() => void) | undefined;
  void (async () => {
    try {
      const unlisten = await options.listen((state) => {
        if (!disposed) options.receive(state);
      });
      if (disposed) {
        unlisten();
        return;
      }
      stop = unlisten;
      const state = await options.snapshot();
      if (!disposed) options.receive(state);
    } catch (error) {
      if (!disposed) options.error(error);
    } finally {
      if (!disposed) options.ready();
    }
  })();
  return () => {
    disposed = true;
    stop?.();
  };
}
