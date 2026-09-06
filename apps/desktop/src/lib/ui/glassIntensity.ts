export type GlassIntensity = number;

export const GLASS_INTENSITY_MIN = 0;
export const GLASS_INTENSITY_MAX = 100;
export const DEFAULT_GLASS_INTENSITY = 50;

const GLASS_INTENSITY_KEY = "astro-glass-intensity";

export function normalizeGlassIntensity(value: unknown): GlassIntensity {
  if (value === null || value === undefined || value === "") {
    return DEFAULT_GLASS_INTENSITY;
  }
  const numeric = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(numeric)) return DEFAULT_GLASS_INTENSITY;
  return Math.min(
    GLASS_INTENSITY_MAX,
    Math.max(GLASS_INTENSITY_MIN, Math.round(numeric)),
  );
}

export function readStoredGlassIntensity(
  storage: Pick<Storage, "getItem"> = localStorage,
): GlassIntensity {
  try {
    return normalizeGlassIntensity(storage.getItem(GLASS_INTENSITY_KEY));
  } catch {
    return DEFAULT_GLASS_INTENSITY;
  }
}

export function persistGlassIntensity(
  intensity: GlassIntensity,
  storage: Pick<Storage, "setItem"> = localStorage,
) {
  const normalized = normalizeGlassIntensity(intensity);
  try {
    storage.setItem(GLASS_INTENSITY_KEY, String(normalized));
  } catch {
    // Storage can be unavailable in privacy-restricted webviews.
  }
}

export function applyGlassIntensity(
  root: HTMLElement,
  intensity: GlassIntensity,
) {
  const normalized = normalizeGlassIntensity(intensity);
  root.removeAttribute("data-glass");
  root.dataset.glassIntensity = String(normalized);
  root.style.setProperty("--glass-intensity", String(normalized / 100));
}
