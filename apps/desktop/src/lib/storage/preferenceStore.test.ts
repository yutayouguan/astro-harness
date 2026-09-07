import assert from "node:assert/strict";
import test from "node:test";

import {
  emitPreferenceChange,
  readPreference,
  removePreference,
  writePreference,
} from "./preferenceStore.ts";

test("preference reads decode values and fail closed to the fallback", () => {
  assert.equal(
    readPreference("theme", "auto", (raw) => raw.toUpperCase(), {
      getItem: () => "dark",
    }),
    "DARK",
  );
  assert.equal(
    readPreference(
      "theme",
      "auto",
      () => {
        throw new Error("invalid");
      },
      { getItem: () => "broken" },
    ),
    "auto",
  );
  assert.equal(readPreference("theme", "auto", String, null), "auto");
});

test("preference writes and removals report unavailable storage", () => {
  const entries = new Map<string, string>();
  const storage = {
    setItem: (key: string, value: string) => entries.set(key, value),
    removeItem: (key: string) => entries.delete(key),
  };

  assert.equal(writePreference("scale", 92, String, storage), true);
  assert.equal(entries.get("scale"), "92");
  assert.equal(removePreference("scale", storage), true);
  assert.equal(entries.has("scale"), false);
  assert.equal(writePreference("scale", 92, String, null), false);
  assert.equal(removePreference("scale", null), false);
});

test("preference change events keep the typed detail payload", () => {
  let received: Event | null = null;
  const target = {
    dispatchEvent(event: Event) {
      received = event;
      return true;
    },
  };

  assert.equal(emitPreferenceChange("astro:test", { value: 64 }, target), true);
  assert.equal((received as CustomEvent<{ value: number }>).detail.value, 64);
});
