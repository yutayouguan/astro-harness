import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const read = (path) => readFile(new URL(path, import.meta.url), "utf8");
const [css, main, theme, picker, preferences, layout, runtime] =
  await Promise.all([
    read("../../styles/materials/soft.css"),
    read("../../main.tsx"),
    read("../../hooks/app/useTheme.tsx"),
    read("../../components/settings/InterfaceMaterialPicker.tsx"),
    read("../../components/settings/PreferencesPanel.tsx"),
    read("../../styles/features/preferences.css"),
    read("./interfaceMaterial.ts"),
  ]);

test("soft material defines separate readable light and dark surfaces", () => {
  for (const mode of ["light", "dark"]) {
    assert.ok(css.includes(`html[data-material="soft"][data-theme="${mode}"]`));
  }
  const luminance = (hex) => {
    const channels = hex.match(/\w\w/g).map((value) => {
      const c = parseInt(value, 16) / 255;
      return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    });
    return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
  };
  for (const block of css.matchAll(
    /\[data-theme="(?:light|dark)"\]\s*\{([^}]+)\}/g,
  )) {
    const color = (name) =>
      block[1].match(new RegExp(`--soft-${name}: #([0-9a-f]{6});`))[1];
    for (const text of ["ink", "muted"]) {
      for (const surface of ["base", "surface", "inset"]) {
        const a = luminance(color(text));
        const b = luminance(color(surface));
        assert.ok(
          (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05) >= 4.5,
          `${text}/${surface}`,
        );
      }
    }
  }
});

test("material prewarms before React mount and does not color companion canvases", () => {
  assert.ok(
    main.indexOf("readStoredInterfaceMaterial(),") <
      main.indexOf("ReactDOM.createRoot"),
  );
  assert.match(main, /if \(!isDesktopPetWindow && !isPetTaskWindow\)/);
  assert.match(
    theme,
    /applyInterfaceMaterial\(document.documentElement, material\)/,
  );
  assert.match(theme, /persistInterfaceMaterial\(normalized\)/);
  assert.doesNotMatch(
    runtime,
    /wallpaper|glass-intensity|theme-mode|reset_active/,
  );
});

test("picker has native keyboard radios and glass strength is disabled, not discarded", () => {
  assert.match(picker, /<fieldset/);
  assert.match(picker, /type="radio"/);
  assert.match(picker, /checked=\{material === id\}/);
  assert.match(
    preferences,
    /id="appearance-glass-intensity"\s+disabled=\{material === "soft"\}/,
  );
  assert.match(preferences, /prefs.appearance.glass.softDisabled/);
});

test("material preserves approval composition, focus, reduced motion and contrast", () => {
  assert.match(css, /\.composer:not\(\.has-clarify\)/);
  assert.match(
    css,
    /\.composer-policy-pill\.is-full-access\s*\{[\s\S]*?border-color: var\(--tone-orange\)/,
  );
  assert.match(css, /:focus-visible/);
  assert.match(css, /prefers-reduced-motion: reduce/);
  assert.match(css, /prefers-contrast: more/);
  assert.match(
    css,
    /@media \(prefers-contrast: more\)\s*\{\s*html\[data-material="soft"\]\[data-theme\]/,
  );
  assert.match(css, /--immersive-reference-backdrop: none/);
  assert.doesNotMatch(css, /!important|--tone:|--accent:/);
  assert.match(
    layout,
    /@container \(max-width: 760px\)[\s\S]*?grid-template-columns: minmax\(0, 1fr\)/,
  );
});
