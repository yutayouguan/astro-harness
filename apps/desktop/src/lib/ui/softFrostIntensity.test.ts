import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import {
  applySoftFrostIntensity,
  normalizeSoftFrostIntensity,
  persistSoftFrostIntensity,
  readStoredSoftFrostIntensity,
  softFrostParameters,
} from "./softFrostIntensity.ts";

test("frost clamps, rounds and rejects malformed stored values", () => {
  for (const value of [
    null,
    undefined,
    "",
    "  ",
    NaN,
    Infinity,
    "bad",
    {},
    true,
  ])
    assert.equal(normalizeSoftFrostIntensity(value), 50);
  assert.equal(normalizeSoftFrostIntensity(-4), 0);
  assert.equal(normalizeSoftFrostIntensity(105), 100);
  assert.equal(normalizeSoftFrostIntensity("32.6"), 33);
});

test("one range links opacity and blur without changing foreground opacity", () => {
  assert.deepEqual(softFrostParameters(0), { opacity: 100, blur: 0 });
  assert.deepEqual(softFrostParameters(50), { opacity: 79, blur: 32 });
  assert.deepEqual(softFrostParameters(100), { opacity: 72, blur: 64 });
  const properties = new Map<string, string>();
  const root = {
    dataset: {} as Record<string, string>,
    style: {
      setProperty: (key: string, value: string) => properties.set(key, value),
    },
  };
  applySoftFrostIntensity(root as unknown as HTMLElement, 50);
  assert.equal(root.dataset.softFrostIntensity, "50");
  assert.deepEqual(
    [...properties],
    [
      ["--soft-frost-opacity", "79%"],
      ["--soft-frost-blur", "32px"],
    ],
  );
});

test("frost persists independently of Glass and wallpaper and tolerates unavailable storage", () => {
  const wallpaper = JSON.stringify({ blur: 7, shade: 34, mode: "wallpaper" });
  const values = new Map([
    ["astro-glass-intensity", "23"],
    ["astro-wallpaper-prefs.v1", wallpaper],
  ]);
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
  };
  assert.equal(readStoredSoftFrostIntensity(storage), 50);
  persistSoftFrostIntensity(81, storage);
  assert.equal(readStoredSoftFrostIntensity(storage), 81);
  assert.equal(values.get("astro-glass-intensity"), "23");
  assert.equal(values.get("astro-wallpaper-prefs.v1"), wallpaper);
  assert.equal(
    readStoredSoftFrostIntensity({
      getItem() {
        throw new Error("denied");
      },
    }),
    50,
  );
  assert.doesNotThrow(() =>
    persistSoftFrostIntensity(70, {
      setItem() {
        throw new Error("denied");
      },
    }),
  );
});

test("every slider step increases blur and transmission within readable bounds", () => {
  let previous = softFrostParameters(0);
  for (let step = 1; step <= 100; step++) {
    const current = softFrostParameters(step);
    assert.ok(current.opacity < previous.opacity);
    assert.ok(current.opacity >= 72 && current.opacity <= 100);
    assert.ok(current.blur > previous.blur && current.blur <= 64);
    previous = current;
  }
});

test("global frost has solid fallbacks and settings/pet cards consume the same neutral recipe", async () => {
  const read = (path: string) =>
    readFile(new URL(path, import.meta.url), "utf8");
  const [css, settings, pets, theme, main] = await Promise.all([
    read("../../styles/materials/soft.css"),
    read("../../styles/features/settings-material-unified.css"),
    read("../../styles/features/pet-material.css"),
    read("../../hooks/app/useTheme.tsx"),
    read("../../main.tsx"),
  ]);
  assert.match(css, /@supports \(backdrop-filter/);
  assert.match(css, /var\(--soft-frost-opacity, 79%\)/);
  assert.match(
    css,
    /prefers-reduced-transparency: reduce[\s\S]*--soft-material-background: var\(--soft-surface\);\s*--soft-material-backdrop: none/,
  );
  assert.match(css, /forced-colors: active/);
  const index = await read("../../styles/index.css");
  assert.match(index, /@import "\.\/materials\/soft-focus.css";/);
  assert.match(
    index,
    /@import "\.\/features\/pet-material.css" layer\(features\);/,
  );
  assert.doesNotMatch(
    await read("../../components/settings/DesktopPetPanel.tsx"),
    /import .*pet-material.css/,
  );
  assert.match(
    settings,
    /:is\(\.prefs-card, \.prefs-section, \.pet-manager-commandbar\)/,
  );
  assert.match(
    pets,
    /html\[data-material="soft"\] \.pet-manager\s*\{\s*--pet-material-surface: var\(--soft-material-background\)/,
  );
  assert.equal(
    theme.match(
      /applySoftFrostIntensity\(document.documentElement, softFrostIntensity\)/g,
    )?.length,
    2,
  );
  assert.ok(
    main.indexOf("readStoredSoftFrostIntensity()") <
      main.indexOf("ReactDOM.createRoot"),
  );
});
