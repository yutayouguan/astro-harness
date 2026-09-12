import type { PetMotionClips } from "./petMotionClip";

export type PetActionSource = {
  spriteVersionNumber: number | null;
  motionClips?: PetMotionClips;
  groomingPath?: string | null;
};

const LABELS: Record<string, { zh: string; en: string }> = {
  idle: { zh: "待机 / 眨眼", en: "Idle / blink" },
  kneading: { zh: "踩奶", en: "Knead" },
  grooming: { zh: "舔脚脚", en: "Groom" },
  "tail-wag": { zh: "摇尾巴", en: "Wag tail" },
  "head-tilt": { zh: "歪头", en: "Tilt head" },
  stretch: { zh: "伸懒腰", en: "Stretch" },
  nap: { zh: "趴下打盹", en: "Nap" },
};

export function petActionLabel(name: string, locale: "zh" | "en") {
  return LABELS[name]?.[locale] ?? name;
}

/** A single capability list for previews, context menus and auto-play. Unknown
 * imported clip names are playable too; a dog never inherits cat actions. */
export function availablePetActions(source: PetActionSource): string[] {
  if (![2, 3].includes(source.spriteVersionNumber ?? 0)) return [];
  const names = new Set(Object.keys(source.motionClips ?? {}));
  // The legacy six-frame grooming extension also signals the old kneading row.
  if (source.groomingPath) {
    names.add("kneading");
    names.add("grooming");
  }
  names.delete("idle");
  // v3 keeps lifecycle animations and gaze in the same APNG map; they are not
  // leisure activities to be randomly played while the agent is idle.
  if (source.spriteVersionNumber === 3) {
    for (const name of [
      "look",
      "running-right",
      "running-left",
      "waving",
      "jumping",
      "failed",
      "waiting",
      "running",
      "review",
    ])
      names.delete(name);
  }
  const order = Object.keys(LABELS);
  return [...names].sort((a, b) => {
    const ai = order.indexOf(a),
      bi = order.indexOf(b);
    return (
      (ai < 0 ? order.length : ai) - (bi < 0 ? order.length : bi) ||
      a.localeCompare(b)
    );
  });
}

export function nextPetAction(
  available: readonly string[],
  previous: string | null,
) {
  if (!available.length) return null;
  return available[(available.indexOf(previous ?? "") + 1) % available.length];
}

export function availablePetActionSignature(source: PetActionSource) {
  return JSON.stringify({
    petActions: availablePetActions(source),
    clips: source.motionClips ?? {},
    groomingPath: source.groomingPath,
  });
}
