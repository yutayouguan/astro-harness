import type { PetMotionClip, PetMotionClips } from "./petMotionClip";
import type { PetDefaults } from "./petLibrary";
import type { PetPreferences } from "./petPreferences";

/** Capture only the confirmed ground anchor; preserve other unsaved defaults. */
export function withGroundPlacement(draft: PetDefaults, preferences: PetPreferences | undefined): PetDefaults {
  if (!preferences?.position || !preferences.roamingEnabled) {
    throw new Error("未能确认底部位置，请重试 / Ground placement was not confirmed");
  }
  return {
    ...draft,
    behavior: {
      ...draft.behavior,
      position: { ...preferences.position },
      roamingEnabled: true,
    },
  };
}
export type PetRoamingFrame = {
  generation: number;
  active: boolean;
  returning: boolean;
  clipName: string | null;
  clip: PetMotionClip | null;
  elapsedMs: number;
  petId: string | null;
  revision: number;
};
export function supportsPetRoaming(clips: PetMotionClips | undefined) {
  return ["running-left", "running-right"].every((name) => {
    const clip = clips?.[name];
    if (!clip || !Number.isInteger(clip.loopStart) || !Number.isInteger(clip.loopEnd)) return false;
    const stride = clip.locomotion?.stridePx ?? 0;
    const body = clip.durationsMs.slice(clip.loopStart, clip.loopEnd);
    const sum = (values: number[]) => values.reduce((total, value) => total + value, 0);
    return (
      clip.path.endsWith(".apng") &&
      Number.isInteger(stride) && stride >= 4 && stride <= 384 &&
      clip.loopStart > 0 && clip.loopEnd < clip.durationsMs.length &&
      body.length >= 4 &&
      clip.durationsMs.every((duration) => Number.isInteger(duration) && duration > 0) &&
      sum(body) >= 400 && sum(body) <= 3000 &&
      sum(clip.durationsMs.slice(0, clip.loopStart)) <= 2000 &&
      sum(clip.durationsMs.slice(clip.loopEnd)) <= 2000
    );
  });
}
export function acceptRoamingFrame(
  current: PetRoamingFrame | null,
  next: PetRoamingFrame,
) {
  if (
    !Number.isSafeInteger(next.generation) ||
    next.generation < (current?.generation ?? -1) ||
    !Number.isFinite(next.elapsedMs) ||
    next.elapsedMs < 0
  )
    return current;
  if (current && next.generation === current.generation) {
    if (!current.active || next.elapsedMs < current.elapsedMs) return current;
    return { ...next, clip: current.clip };
  }
  return next;
}
export function roamingInterval(activitySecs: number, random: number) {
  return (
    Math.max(60, activitySecs * 2) *
    1000 *
    (0.8 + Math.max(0, Math.min(1, random)) * 0.4)
  );
}
