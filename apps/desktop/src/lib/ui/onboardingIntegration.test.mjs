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
const app = readFileSync(new URL("../../App.tsx", import.meta.url), "utf8");

test("desktop startup is gated by the persisted first-run flow", () => {
  assert.match(
    main,
    /<OnboardingGate>[\s\S]*?<App \/>[\s\S]*?<\/OnboardingGate>/,
  );
  assert.match(gate, /invoke<OnboardingStateDto>\("get_onboarding_state"\)/);
  assert.match(gate, /"complete_onboarding"/);
  assert.match(gate, /verificationToken: verified\.current\?\.token/);
});

test("onboarding uses canonical provider, project, and permission commands", () => {
  assert.match(gate, /"set_provider_api_key"/);
  assert.match(gate, /"verify_onboarding_provider"/);
  assert.match(gate, /"set_active_provider_model"/);
  assert.match(gate, /"create_project"/);
  assert.match(gate, /"set_permission_preset"/);
  assert.match(gate, /"set_default_agent_name"/);
  assert.doesNotMatch(gate, /localStorage\.setItem\([^)]*api/i);
});

test("motion and glass provide accessibility fallbacks", () => {
  assert.match(gate, /useReducedMotion/);
  assert.match(styles, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(styles, /@media \(prefers-reduced-transparency: reduce\)/);
  assert.match(styles, /@media \(prefers-contrast: more\)/);
});

test("provider picker uses the shared SelectMenu and no native select", () => {
  assert.match(gate, /<SelectMenu/);
  assert.match(gate, /onChange=\{selectProvider\}/);
  assert.doesNotMatch(gate, /<select\b/);
  assert.doesNotMatch(styles, /onboarding-select-wrap/);
});

test("registered commands support progress, completion, and settings reset", () => {
  for (const command of [
    "get_onboarding_state",
    "verify_onboarding_provider",
    "save_onboarding_progress",
    "complete_onboarding",
    "reset_onboarding_state",
  ]) {
    assert.match(commands, new RegExp(`commands::onboarding::${command}`));
  }
  assert.match(preferences, /ONBOARDING_RESET_EVENT/);
  assert.match(preferences, /"reset_onboarding_state"/);
  assert.match(gate, /storeOnboardingStarterPrompt/);
  assert.match(commands, /commands::agent::set_default_agent_name/);
  assert.match(app, /takeOnboardingStarterPrompt\(\)/);
  assert.match(app, /startNewChat\(\)\.then/);
  assert.match(app, /setInput\(starterPrompt\)/);
});

test("draft disk writes are dispatched off the UI thread", () => {
  const native = readFileSync(new URL("../../../src-tauri/src/commands/ui/onboarding.rs", import.meta.url), "utf8");
  assert.match(native, /pub async fn save_onboarding_progress[\s\S]*?spawn_blocking\(move \|\| save_step_at/);
});
