import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

test("offline built-in pet has a Settings action, embedded assets and native route", () => {
  const panel = read("../../components/settings/PetLibraryPanel.tsx");
  const commands = read("../../../src-tauri/src/lib.rs");
  const builtin = read("../../../src-tauri/src/commands/ui/builtin_pet.rs");
  const onboarding = read(
    "../../../src-tauri/src/commands/ui/desktop_preferences.rs",
  );
  assert.match(panel, /apply_library_pet/);
  assert.match(panel, /item\.builtin/);
  assert.match(builtin, /ensure_library/);
  assert.match(commands, /commands::desktop_pet::use_builtin_desktop_pet/);
  assert.match(builtin, /include_bytes!\([^;]*spritesheet\.webp/s);
  assert.match(builtin, /include_bytes!\([^;]*grooming\.webp/s);
  assert.match(onboarding, /builtin_pet::install_into_state/);
  assert.doesNotMatch(onboarding, /include_bytes!.*onboarding-companion\.svg/);
});

test("pet settings use defined theme-aware surfaces instead of white fallbacks", () => {
  const css = read("../../styles/features/desktop-pet.css");
  assert.doesNotMatch(css, /--ink-muted/);
  const material = read("../../styles/features/settings-material-unified.css");
  for (const token of [
    "settings-panel-border",
    "settings-panel-background",
    "settings-panel-shadow",
    "settings-inset-background",
  ]) {
    assert.ok(css.includes(`var(--${token}`));
    assert.ok(material.includes(`--${token}:`));
  }
  for (const name of [
    "color-bg-subtle",
    "color-text",
    "color-text-secondary",
    "color-border",
  ]) {
    for (const theme of ["light", "dark"]) {
      assert.match(
        read(`../../styles/tokens/semantic-${theme}.css`),
        new RegExp(`--${name}:`),
      );
    }
  }
  assert.match(
    css,
    /\.desktop-pet-hero::after\s*\{[^}]*var\(--color-bg-subtle\) 42%/s,
  );
  assert.match(css, /\.desktop-pet-preview\s*\{[^}]*var\(--color-bg-subtle\)/s);
  assert.match(css, /::placeholder\s*\{[^}]*var\(--color-text-muted\)/s);
});
