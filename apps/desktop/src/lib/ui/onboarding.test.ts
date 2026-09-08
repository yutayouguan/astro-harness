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
  classifyConnectionIssue,
  createOnboardingWriteQueue,
  persistableOnboardingEndpoint,
  withDeadline,
  resumeOnboardingStep,
} from "./onboarding.ts";
import { buildStarterPrompt } from "./onboardingTasks.ts";

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
  assert.equal(resumeOnboardingStep("workspace"), "provider");
  assert.equal(resumeOnboardingStep("personalize"), "personalize");
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

test("connection failures classify quota before generic 429 and never expose raw errors", () => {
  assert.equal(classifyConnectionIssue("429 insufficient_quota"), "quota");
  assert.equal(
    classifyConnectionIssue("401 invalid API key=secret"),
    "credentials",
  );
  assert.equal(classifyConnectionIssue("404 model_not_found"), "model");
  assert.equal(classifyConnectionIssue("request timed out"), "timeout");
  assert.equal(classifyConnectionIssue("fetch failed"), "network");
  assert.equal(classifyConnectionIssue("429 rate limit"), "rate_limit");
  assert.equal(classifyConnectionIssue("unexpected"), "unknown");
});

test("small tasks require bounded input and preserve the exact text", () => {
  assert.equal(buildStarterPrompt("summarize", " ", "zh"), null);
  assert.equal(buildStarterPrompt("explain", "x".repeat(4001), "en"), null);
  assert.match(
    buildStarterPrompt("summarize", "line 1\nline 2", "zh")!,
    /三个简明要点/,
  );
  assert.ok(
    buildStarterPrompt("explain", "const a = 1", "en")!.endsWith("const a = 1"),
  );
  assert.match(buildStarterPrompt("translate", "你好", "en")!, /Translate/);
});

test("credential-bearing and invalid endpoints are excluded from drafts", () => {
  for (const endpoint of [
    "sk-test-value",
    "https://user:pass@example.com",
    "https://example.com?api_key=secret",
    "https://example.com?token=secret",
    "file:///tmp/x",
  ]) {
    assert.equal(persistableOnboardingEndpoint(endpoint), "");
  }
  assert.equal(
    persistableOnboardingEndpoint("https://example.com/v1"),
    "https://example.com/v1",
  );
});

test("progress writes and completion remain ordered, even after failure", async () => {
  const queue = createOnboardingWriteQueue();
  const events: string[] = [];
  let release!: () => void;
  const deferred = new Promise<void>((resolve) => {
    release = resolve;
  });
  const save = queue.run(async () => {
    await deferred;
    events.push("saved");
  });
  const fail = queue.run(async () => {
    events.push("failed");
    throw new Error("disk");
  });
  const caught = fail.catch(() => undefined);
  const finish = queue.run(async () => {
    events.push("complete");
  });
  assert.deepEqual(events, []);
  release();
  await Promise.all([save, caught, finish]);
  assert.deepEqual(events, ["saved", "failed", "complete"]);
});

test("connection deadline rejects a hung request", async () => {
  await assert.rejects(withDeadline(new Promise(() => {}), 2), /timeout/);
  assert.equal(await withDeadline(Promise.resolve(42)), 42);
});

test("first draft survives unavailable session storage", () => {
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      get sessionStorage() {
        throw new Error("storage denied");
      },
    },
  });
  try {
    storeOnboardingStarterPrompt("offline draft");
    assert.equal(takeOnboardingStarterPrompt(), "offline draft");
    assert.equal(takeOnboardingStarterPrompt(), null);
  } finally {
    if (descriptor) Object.defineProperty(globalThis, "window", descriptor);
    else Reflect.deleteProperty(globalThis, "window");
  }
});
