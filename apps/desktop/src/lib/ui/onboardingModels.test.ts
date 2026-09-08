import assert from "node:assert/strict";
import { test } from "node:test";
import { onboardingModelOptions } from "./onboardingModels.ts";

test("only returned chat models are selectable, with stable deduplication", () => {
  assert.deepEqual(
    onboardingModelOptions([
      { id: " qa-small ", display_name: "Small" },
      { id: "qa-small" },
      { id: "qa-alt" },
      { id: "" },
      { id: "text-embedding-3-small" },
      { id: "whisper-1" },
      { id: "gpt-image-2" },
      { id: "veo-3" },
      { id: "sora-2" },
    ]),
    [
      { value: "qa-small", label: "Small · qa-small" },
      { value: "qa-alt", label: "qa-alt" },
    ],
  );
});

test("empty or media-only responses never invent a default model", () => {
  assert.deepEqual(onboardingModelOptions([]), []);
  assert.deepEqual(onboardingModelOptions([{ id: "tts-1" }]), []);
});
