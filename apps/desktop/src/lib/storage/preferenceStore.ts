export type PreferenceStorage = Pick<
  Storage,
  "getItem" | "setItem" | "removeItem"
>;

function defaultStorage(): PreferenceStorage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

export function readPreference<T>(
  key: string,
  fallback: T,
  decode: (raw: string) => T,
  storage: Pick<PreferenceStorage, "getItem"> | null = defaultStorage(),
): T {
  if (!storage) return fallback;
  try {
    const raw = storage.getItem(key);
    return raw == null ? fallback : decode(raw);
  } catch {
    return fallback;
  }
}

export function writePreference<T>(
  key: string,
  value: T,
  encode: (value: T) => string = String,
  storage: Pick<PreferenceStorage, "setItem"> | null = defaultStorage(),
): boolean {
  if (!storage) return false;
  try {
    storage.setItem(key, encode(value));
    return true;
  } catch {
    return false;
  }
}

export function removePreference(
  key: string,
  storage: Pick<PreferenceStorage, "removeItem"> | null = defaultStorage(),
): boolean {
  if (!storage) return false;
  try {
    storage.removeItem(key);
    return true;
  } catch {
    return false;
  }
}

export function emitPreferenceChange<T>(
  eventName: string,
  detail: T,
  target: Pick<Window, "dispatchEvent"> | null = typeof window === "undefined"
    ? null
    : window,
): boolean {
  if (!target || typeof CustomEvent === "undefined") return false;
  return target.dispatchEvent(new CustomEvent<T>(eventName, { detail }));
}
