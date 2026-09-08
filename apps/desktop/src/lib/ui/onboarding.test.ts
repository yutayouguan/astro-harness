import assert from "node:assert/strict";
import { test } from "node:test";
import type { ProviderDto } from "../../types.ts";
import {
  inferProjectName,
  normalizeOnboardingStep,
  providerConfigInput,
  providerIsReady,
  providerRequiresApiKey,
  storeOnboardingStarterPrompt,
  takeOnboardingStarterPrompt,
} from "./onboarding.ts";

const provider: ProviderDto = {
  id: "openai-main",
  kind: "openai",
  display_name: "OpenAI",
  endpoint: "https://api.openai.com/v1",
  model: "gpt-5.6",
  enabled: true,
  has_api_key: true,
  key_source: "keyring",
  env_key_name: "OPENAI_API_KEY",
  backend_id: "openai",
  supports_responses_api: true,
};

test("normalizes persisted onboarding steps", () => {
  assert.equal(normalizeOnboardingStep("provider"), "provider");
  assert.equal(normalizeOnboardingStep("future-step"), "intro");
});

test("requires credentials only for providers that need them", () => {
  assert.equal(providerRequiresApiKey(provider), true);
  assert.equal(
    providerRequiresApiKey({
      ...provider,
      kind: "ollama",
      key_source: "not_required",
    }),
    false,
  );
});

test("provider readiness requires native Responses support", () => {
  assert.equal(providerIsReady(provider), true);
  assert.equal(
    providerIsReady({ ...provider, supports_responses_api: false }),
    false,
  );
  assert.equal(providerIsReady({ ...provider, has_api_key: false }), false);
});

test("provider save payload preserves capability-specific models", () => {
  const input = providerConfigInput(
    { ...provider, image_model: "gpt-image-2", fallback: [] },
    " gpt-5.6-sol ",
  );
  assert.equal(input.model, "gpt-5.6-sol");
  assert.equal(input.image_model, "gpt-image-2");
  assert.equal(input.enabled, true);
});

test("infers project names on unix and windows paths", () => {
  assert.equal(inferProjectName("/Users/me/code/astro"), "astro");
  assert.equal(inferProjectName("C:\\work\\nova"), "nova");
  assert.equal(inferProjectName(""), "Workspace");
});

test("starter prompt is consumed exactly once", () => {
  const values = new Map<string, string>();
  const previousWindow = globalThis.window;
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      sessionStorage: {
        setItem: (key: string, value: string) => values.set(key, value),
        getItem: (key: string) => values.get(key) ?? null,
        removeItem: (key: string) => values.delete(key),
      },
    },
  });
  try {
    storeOnboardingStarterPrompt("  Review this project  ");
    assert.equal(takeOnboardingStarterPrompt(), "Review this project");
    assert.equal(takeOnboardingStarterPrompt(), null);
  } finally {
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: previousWindow,
    });
  }
});
