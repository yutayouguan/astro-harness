import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import {
  applyInterfaceScale,
  normalizeInterfaceScale,
  persistInterfaceScale,
  readStoredInterfaceScale,
} from "./interfaceScale.ts";

const [preferences, theme, main] = await Promise.all(
  [
    "../../components/settings/PreferencesPanel.tsx",
    "../../hooks/app/useTheme.tsx",
    "../../main.tsx",
  ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
);

test("interface scale is a persisted 80-100 range restored before render", () => {
  assert.equal(readStoredInterfaceScale({ getItem: () => null }), 100);
  assert.equal(normalizeInterfaceScale(72), 80);
  assert.equal(normalizeInterfaceScale(91.6), 92);
  assert.equal(normalizeInterfaceScale(140), 100);
  assert.equal(normalizeInterfaceScale("compact"), 100);
  assert.match(theme, /interfaceScale:\s*InterfaceScale/);
  assert.match(theme, /persistInterfaceScale\(normalized\)/);
  assert.match(main, /readStoredInterfaceScale\(window\.localStorage\)/);
});

test("interface scale persists only its normalized percentage", () => {
  const writes = [];
  persistInterfaceScale(77, {
    setItem(key, value) {
      writes.push([key, value]);
    },
  });
  assert.deepEqual(writes, [["astro-interface-scale", "80"]]);
});

test("interface scale applies one proportional root zoom contract", () => {
  const properties = new Map();
  const root = {
    dataset: {},
    style: {
      setProperty(name, value) {
        properties.set(name, value);
      },
    },
  };

  applyInterfaceScale(root, 85);

  assert.equal(root.dataset.interfaceScale, "85");
  assert.equal(properties.get("--interface-scale"), "0.85");
  assert.equal(properties.get("zoom"), "0.85");
});

test("appearance exposes one accessible interface scale slider", () => {
  const control = preferences.slice(
    preferences.indexOf('id="appearance-interface-scale"'),
    preferences.indexOf('id="appearance-glass-intensity"'),
  );

  assert.match(control, /type="range"/);
  assert.match(control, /min=\{INTERFACE_SCALE_MIN\}/);
  assert.match(control, /max=\{INTERFACE_SCALE_MAX\}/);
  assert.match(control, /step=\{1\}/);
  assert.match(control, /value=\{interfaceScale\}/);
  assert.match(control, /aria-describedby="appearance-scale-description"/);
  assert.match(
    control,
    /setInterfaceScale\(Number\(event\.currentTarget\.value\)\)/,
  );
  assert.match(control, /\{interfaceScale\}%/);
});
