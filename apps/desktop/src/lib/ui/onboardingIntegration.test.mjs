import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const main = readFileSync(new URL("../../main.tsx", import.meta.url), "utf8");
const gate = readFileSync(
  new URL("../../components/onboarding/OnboardingGate.tsx", import.meta.url),
  "utf8",
);
const styles = readFileSync(
  new URL("../../styles/features/onboarding.css", import.meta.url),
  "utf8",
);
const commands = readFileSync(
  new URL("../../../src-tauri/src/lib.rs", import.meta.url),
  "utf8",
);
const preferences = readFileSync(
  new URL("../../components/settings/PreferencesPanel.tsx", import.meta.url),
  "utf8",
);

test("desktop startup is gated by the persisted first-run flow", () => {
  assert.match(
    main,
    /<OnboardingGate>[\s\S]*?<App \/>[\s\S]*?<\/OnboardingGate>/,
  );
  assert.match(gate, /invoke<OnboardingStateDto>\("get_onboarding_state"\)/);
  assert.match(gate, /invoke<OnboardingStateDto>\("complete_onboarding"\)/);
});

test("onboarding uses canonical provider, project, and permission commands", () => {
  assert.match(gate, /"set_provider_api_key"/);
  assert.match(gate, /"test_provider"/);
  assert.match(gate, /"set_active_provider_model"/);
  assert.match(gate, /"create_project"/);
  assert.match(gate, /"set_permission_preset"/);
  assert.doesNotMatch(gate, /localStorage\.setItem\([^)]*api/i);
});

test("motion and glass provide accessibility fallbacks", () => {
  assert.match(gate, /useReducedMotion/);
  assert.match(styles, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(styles, /@media \(prefers-reduced-transparency: reduce\)/);
  assert.match(styles, /@media \(prefers-contrast: more\)/);
});

test("registered commands support progress, completion, and settings reset", () => {
  for (const command of [
    "get_onboarding_state",
    "save_onboarding_progress",
    "complete_onboarding",
    "reset_onboarding_state",
  ]) {
    assert.match(commands, new RegExp(`commands::onboarding::${command}`));
  }
  assert.match(preferences, /ONBOARDING_RESET_EVENT/);
  assert.match(preferences, /"reset_onboarding_state"/);
});
