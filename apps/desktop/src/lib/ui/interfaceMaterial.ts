import { readPreference, writePreference } from "../storage/preferenceStore.ts";

export type InterfaceMaterial = "glass" | "soft";
export const DEFAULT_INTERFACE_MATERIAL: InterfaceMaterial = "glass";
const STORAGE_KEY = "astro-interface-material";

export function normalizeInterfaceMaterial(value: unknown): InterfaceMaterial {
  return value === "soft" ? "soft" : DEFAULT_INTERFACE_MATERIAL;
}

export function readStoredInterfaceMaterial(
  storage?: Pick<Storage, "getItem">,
) {
  return readPreference(
    STORAGE_KEY,
    DEFAULT_INTERFACE_MATERIAL,
    normalizeInterfaceMaterial,
    storage,
  );
}

export function persistInterfaceMaterial(
  material: InterfaceMaterial,
  storage?: Pick<Storage, "setItem">,
) {
  return writePreference(
    STORAGE_KEY,
    normalizeInterfaceMaterial(material),
    String,
    storage,
  );
}

export function applyInterfaceMaterial(
  root: Pick<HTMLElement, "dataset">,
  material: InterfaceMaterial,
) {
  root.dataset.material = normalizeInterfaceMaterial(material);
}
