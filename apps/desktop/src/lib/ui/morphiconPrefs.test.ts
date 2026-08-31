import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_MORPHICON_PREFS,
  MORPHICON_PREFS_KEY,
  normalizeMorphiconPrefs,
  readMorphiconPrefs,
  writeMorphiconPrefs,
} from "./morphiconPrefs.ts";

function installStorage(): Map<string, string> {
  const store = new Map<string, string>();
  (globalThis as { localStorage?: Storage }).localStorage = {
    getItem: (key) => store.get(key) ?? null,
    setItem: (key, value) => {
      store.set(key, value);
    },
    removeItem: (key) => {
      store.delete(key);
    },
    clear: () => store.clear(),
    key: () => null,
    length: 0,
  };
  return store;
}

test("normalizeMorphiconPrefs validates each preference independently", () => {
  assert.deepEqual(normalizeMorphiconPrefs(null), DEFAULT_MORPHICON_PREFS);
  assert.deepEqual(
    normalizeMorphiconPrefs({ spring: "bouncy", strokeWidth: 1.5 }),
    {
      spring: "bouncy",
      strokeWidth: 1.5,
    },
  );
  assert.deepEqual(
    normalizeMorphiconPrefs({ spring: "elastic", strokeWidth: 8 }),
    {
      spring: "smooth",
      strokeWidth: 2,
    },
  );
});

test("morphicon preferences round-trip through localStorage", () => {
  const store = installStorage();
  assert.deepEqual(readMorphiconPrefs(), DEFAULT_MORPHICON_PREFS);

  writeMorphiconPrefs({ spring: "snappy", strokeWidth: 2.5 });
  assert.deepEqual(readMorphiconPrefs(), {
    spring: "snappy",
    strokeWidth: 2.5,
  });
  assert.equal(
    store.get(MORPHICON_PREFS_KEY),
    JSON.stringify({ spring: "snappy", strokeWidth: 2.5 }),
  );

  store.set(MORPHICON_PREFS_KEY, "{bad json");
  assert.deepEqual(readMorphiconPrefs(), DEFAULT_MORPHICON_PREFS);
});
