import { test } from "node:test";
import assert from "node:assert/strict";
import { shouldShowThinkingControls } from "./shouldShowThinkingControls.ts";

test("uses explicit capabilities.reasoning when present", () => {
  assert.equal(
    shouldShowThinkingControls({
      capabilities: {
        vision: false,
        web: false,
        reasoning: true,
        tools: true,
        image_gen: false,
        video_gen: false,
        audio_gen: false,
        music_gen: false,
      },
      backendId: "openai",
    }),
    true,
  );
  assert.equal(
    shouldShowThinkingControls({
      capabilities: {
        vision: false,
        web: false,
        reasoning: false,
        tools: true,
        image_gen: false,
        video_gen: false,
        audio_gen: false,
        music_gen: false,
      },
      backendId: "deepseek",
    }),
    false,
  );
});

test("falls back to deepseek whitelist when capabilities unknown", () => {
  assert.equal(
    shouldShowThinkingControls({ capabilities: null, backendId: "deepseek" }),
    true,
  );
  assert.equal(
    shouldShowThinkingControls({ capabilities: undefined, backendId: "openai" }),
    false,
  );
});
