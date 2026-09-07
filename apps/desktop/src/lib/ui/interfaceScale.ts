import { readPreference, writePreference } from "../storage/preferenceStore.ts";

export type InterfaceScale = number;

export const INTERFACE_SCALE_MIN = 80;
export const INTERFACE_SCALE_MAX = 100;
export const DEFAULT_INTERFACE_SCALE = 100;

const INTERFACE_SCALE_KEY = "astro-interface-scale";

export function normalizeInterfaceScale(value: unknown): InterfaceScale {
  if (value === null || value === undefined || value === "") {
    return DEFAULT_INTERFACE_SCALE;
  }
  const numeric = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(numeric)) return DEFAULT_INTERFACE_SCALE;
  return Math.min(
    INTERFACE_SCALE_MAX,
    Math.max(INTERFACE_SCALE_MIN, Math.round(numeric)),
  );
}

export function readStoredInterfaceScale(
  storage?: Pick<Storage, "getItem">,
): InterfaceScale {
  return readPreference(
    INTERFACE_SCALE_KEY,
    DEFAULT_INTERFACE_SCALE,
    normalizeInterfaceScale,
    storage,
  );
}

export function persistInterfaceScale(
  scale: InterfaceScale,
  storage?: Pick<Storage, "setItem">,
) {
  const normalized = normalizeInterfaceScale(scale);
  writePreference(INTERFACE_SCALE_KEY, normalized, String, storage);
}

export function applyInterfaceScale(root: HTMLElement, scale: InterfaceScale) {
  const normalized = normalizeInterfaceScale(scale);
  const ratio = String(normalized / 100);
  root.dataset.interfaceScale = String(normalized);
  root.style.setProperty("--interface-scale", ratio);
  // Root zoom expands the CSS viewport while shrinking the surface, so text,
  // strokes, icons, and controls retain their existing proportions. DOM bounds
  // also remain visually accurate for native WebView overlays.
  root.style.setProperty("zoom", ratio);
}
