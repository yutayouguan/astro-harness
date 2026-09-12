import { readPreference, writePreference } from "../storage/preferenceStore.ts";

export const DEFAULT_SOFT_FROST_INTENSITY = 50;
const STORAGE_KEY = "astro-soft-frost-intensity";

export function normalizeSoftFrostIntensity(value: unknown): number {
  if (
    (typeof value !== "number" && typeof value !== "string") ||
    (typeof value === "string" && value.trim() === "")
  ) {
    return DEFAULT_SOFT_FROST_INTENSITY;
  }
  const numeric = Number(value);
  return Number.isFinite(numeric)
    ? Math.min(100, Math.max(0, Math.round(numeric)))
    : DEFAULT_SOFT_FROST_INTENSITY;
}

/** Ease into a milky frost, retaining a 72% floor for readable foregrounds. */
export function softFrostParameters(value: unknown) {
  const intensity = normalizeSoftFrostIntensity(value);
  const progress = intensity / 100;
  return {
    opacity: 100 - 28 * (1 - (1 - progress) ** 2),
    blur: intensity * 0.64,
  };
}

export function readStoredSoftFrostIntensity(
  storage?: Pick<Storage, "getItem">,
) {
  return readPreference(
    STORAGE_KEY,
    DEFAULT_SOFT_FROST_INTENSITY,
    normalizeSoftFrostIntensity,
    storage,
  );
}

export function persistSoftFrostIntensity(
  value: number,
  storage?: Pick<Storage, "setItem">,
) {
  writePreference(
    STORAGE_KEY,
    normalizeSoftFrostIntensity(value),
    String,
    storage,
  );
}

export function applySoftFrostIntensity(root: HTMLElement, value: number) {
  const intensity = normalizeSoftFrostIntensity(value);
  const { opacity, blur } = softFrostParameters(intensity);
  root.dataset.softFrostIntensity = String(intensity);
  root.style.setProperty("--soft-frost-opacity", `${opacity}%`);
  root.style.setProperty("--soft-frost-blur", `${blur}px`);
}
