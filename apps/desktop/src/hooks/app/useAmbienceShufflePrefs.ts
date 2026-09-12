import { useEffect, useRef, useState } from "react";
import {
  readPreference,
  writePreference,
} from "../../lib/storage/preferenceStore";
import {
  AMBIENCE_SHUFFLE_KEY,
  DEFAULT_SHUFFLE_PREFS,
  normalizeShufflePrefs,
  type ShufflePrefs,
} from "../../lib/ui/ambienceShuffle";

const read = () =>
  readPreference(AMBIENCE_SHUFFLE_KEY, DEFAULT_SHUFFLE_PREFS, (raw) =>
    normalizeShufflePrefs(JSON.parse(raw)),
  );

/** Independent library preferences: undoing an appearance never rolls back stars or locks. */
export function useAmbienceShufflePrefs() {
  const [prefs, setPrefs] = useState(read);
  const latest = useRef(prefs);
  useEffect(() => {
    const refresh = (event: StorageEvent) => {
      if (event.key !== null && event.key !== AMBIENCE_SHUFFLE_KEY) return;
      latest.current = read();
      setPrefs(latest.current);
    };
    window.addEventListener("storage", refresh);
    return () => window.removeEventListener("storage", refresh);
  }, []);
  const update = (recipe: (current: ShufflePrefs) => ShufflePrefs) => {
    const next = normalizeShufflePrefs(recipe(latest.current));
    if (!writePreference(AMBIENCE_SHUFFLE_KEY, next, JSON.stringify))
      throw new Error("无法保存收藏与换景偏好，请检查本地存储");
    latest.current = next;
    setPrefs(next);
  };
  return { prefs, latest, update };
}
