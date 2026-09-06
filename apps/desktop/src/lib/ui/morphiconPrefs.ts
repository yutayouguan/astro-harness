export const MORPHICON_PREFS_KEY = "astro-morphicons.v1";

export const MORPHICON_SPRINGS = ["smooth", "snappy", "bouncy"] as const;
export type MorphiconSpring = (typeof MORPHICON_SPRINGS)[number];

export const MORPHICON_STROKE_WIDTHS = [1, 2, 2.5] as const;
export type MorphiconStrokeWidth = (typeof MORPHICON_STROKE_WIDTHS)[number];

export type MorphiconPrefs = {
  spring: MorphiconSpring;
  strokeWidth: MorphiconStrokeWidth;
};

export const DEFAULT_MORPHICON_PREFS: MorphiconPrefs = {
  spring: "smooth",
  strokeWidth: 2,
};

export function normalizeMorphiconPrefs(value: unknown): MorphiconPrefs {
  if (!value || typeof value !== "object")
    return { ...DEFAULT_MORPHICON_PREFS };
  const candidate = value as Partial<MorphiconPrefs>;
  const storedStrokeWidth = (value as { strokeWidth?: unknown }).strokeWidth;
  return {
    spring: MORPHICON_SPRINGS.includes(candidate.spring as MorphiconSpring)
      ? (candidate.spring as MorphiconSpring)
      : DEFAULT_MORPHICON_PREFS.spring,
    strokeWidth:
      storedStrokeWidth === 1.5
        ? DEFAULT_MORPHICON_PREFS.strokeWidth
        : MORPHICON_STROKE_WIDTHS.includes(
              candidate.strokeWidth as MorphiconStrokeWidth,
            )
          ? (candidate.strokeWidth as MorphiconStrokeWidth)
          : DEFAULT_MORPHICON_PREFS.strokeWidth,
  };
}

export function readMorphiconPrefs(): MorphiconPrefs {
  try {
    const raw = localStorage.getItem(MORPHICON_PREFS_KEY);
    return raw
      ? normalizeMorphiconPrefs(JSON.parse(raw))
      : { ...DEFAULT_MORPHICON_PREFS };
  } catch {
    return { ...DEFAULT_MORPHICON_PREFS };
  }
}

export function writeMorphiconPrefs(prefs: MorphiconPrefs): void {
  try {
    localStorage.setItem(
      MORPHICON_PREFS_KEY,
      JSON.stringify(normalizeMorphiconPrefs(prefs)),
    );
  } catch {
    // localStorage may be unavailable in browser previews.
  }
}
