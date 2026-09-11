import assert from "node:assert/strict";
import { test } from "node:test";
import {
  applyInterfaceMaterial,
  normalizeInterfaceMaterial,
  persistInterfaceMaterial,
  readStoredInterfaceMaterial,
} from "./interfaceMaterial.ts";

test("existing installs and invalid material values retain glass", () => {
  for (const value of [null, undefined, "", "neumorphic", {}, 1, "glass"]) {
    assert.equal(normalizeInterfaceMaterial(value), "glass");
  }
  assert.equal(normalizeInterfaceMaterial("soft"), "soft");
  assert.equal(readStoredInterfaceMaterial({ getItem: () => null }), "glass");
  assert.equal(
    readStoredInterfaceMaterial({
      getItem: () => {
        throw Error("denied");
      },
    }),
    "glass",
  );
});

test("material roundtrip touches only its own preference", () => {
  const values = new Map([
    ["astro-glass-intensity", "82"],
    ["astro-theme-mode", "auto"],
    ["astro-wallpaper", "existing-wallpaper"],
  ]);
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
  };
  assert.equal(persistInterfaceMaterial("soft", storage), true);
  assert.equal(readStoredInterfaceMaterial(storage), "soft");
  persistInterfaceMaterial("glass", storage);
  assert.equal(readStoredInterfaceMaterial(storage), "glass");
  values.delete("astro-interface-material");
  assert.deepEqual(
    [...values],
    [
      ["astro-glass-intensity", "82"],
      ["astro-theme-mode", "auto"],
      ["astro-wallpaper", "existing-wallpaper"],
    ],
  );
});

test("applying material preserves theme, intensity and companion window attributes", () => {
  const root = {
    dataset: {
      theme: "dark",
      glassIntensity: "82",
      windowSurface: "desktop-pet",
    },
  };
  applyInterfaceMaterial(root, "soft");
  assert.deepEqual(root.dataset, {
    theme: "dark",
    glassIntensity: "82",
    windowSurface: "desktop-pet",
    material: "soft",
  });
  applyInterfaceMaterial(root, "glass");
  assert.equal((root.dataset as DOMStringMap).material, "glass");
});

test("unavailable storage does not prevent applying material in this session", () => {
  assert.equal(
    persistInterfaceMaterial("soft", {
      setItem: () => {
        throw Error("quota");
      },
    }),
    false,
  );
  const root = { dataset: {} };
  applyInterfaceMaterial(root, "soft");
  assert.deepEqual(root.dataset, { material: "soft" });
});
