import { test } from "node:test";
import assert from "node:assert/strict";
import {
  loadPickerGlobals,
  savePickerGlobals,
} from "./modelPrefs.ts";

test("loadPickerGlobals forces auto and maxMode off and persists", () => {
  const key = "astro.model.pickerGlobals";
  const store = new Map<string, string>();
  (globalThis as { localStorage: Storage }).localStorage = {
    getItem: (k) => store.get(k) ?? null,
    setItem: (k, v) => {
      store.set(k, String(v));
    },
    removeItem: (k) => {
      store.delete(k);
    },
    clear: () => store.clear(),
    key: () => null,
    length: 0,
  };
  savePickerGlobals({ auto: true, maxMode: true });
  const g = loadPickerGlobals();
  assert.equal(g.auto, false);
  assert.equal(g.maxMode, false);
  const raw = store.get(key);
  assert.ok(raw);
  const parsed = JSON.parse(raw!) as { auto: boolean; maxMode: boolean };
  assert.equal(parsed.auto, false);
  assert.equal(parsed.maxMode, false);
});
