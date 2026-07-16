import { test } from "node:test";
import assert from "node:assert/strict";
import {
  classifyModelTier,
  classifyTask,
  isChatModelId,
  selectAutoModel,
  type AutoModelCandidate,
} from "./autoModelSelect.ts";

const candidates: AutoModelCandidate[] = [
  {
    providerId: "g1",
    providerName: "Google",
    providerKind: "google",
    backendId: "google",
    modelId: "gemini-2.5-flash",
    capabilities: {
      vision: true,
      web: true,
      reasoning: false,
      tools: true,
      image_gen: false,
      video_gen: false,
      audio_gen: false,
    },
  },
  {
    providerId: "g1",
    providerName: "Google",
    providerKind: "google",
    backendId: "google",
    modelId: "gemini-2.5-pro",
    capabilities: {
      vision: true,
      web: true,
      reasoning: true,
      tools: true,
      image_gen: false,
      video_gen: false,
      audio_gen: false,
    },
  },
  {
    providerId: "g1",
    providerName: "Google",
    providerKind: "google",
    backendId: "google",
    modelId: "gemini-2.0-flash-lite",
    capabilities: {
      vision: true,
      web: false,
      reasoning: false,
      tools: true,
      image_gen: false,
      video_gen: false,
      audio_gen: false,
    },
  },
  {
    providerId: "g1",
    providerName: "Google",
    providerKind: "google",
    backendId: "google",
    modelId: "gemini-2.5-flash-preview-tts",
  },
];

test("isChatModelId excludes tts/embed", () => {
  assert.equal(isChatModelId("gemini-2.5-flash"), true);
  assert.equal(isChatModelId("gemini-2.5-flash-preview-tts"), false);
  assert.equal(isChatModelId("text-embedding-3"), false);
});

test("classifyModelTier maps flash/pro/lite", () => {
  assert.equal(classifyModelTier("gemini-2.5-flash"), "flash");
  assert.equal(classifyModelTier("gemini-2.5-pro"), "pro");
  assert.equal(classifyModelTier("gemini-2.0-flash-lite"), "lite");
  assert.equal(classifyModelTier("o3-mini"), "reasoning");
});

test("classifyTask routes vision and short ask", () => {
  assert.equal(
    classifyTask({
      text: "hi",
      hasImages: true,
      chatMode: "ask",
      maxMode: false,
    }),
    "vision",
  );
  assert.equal(
    classifyTask({
      text: "你好",
      hasImages: false,
      chatMode: "ask",
      maxMode: false,
    }),
    "simple",
  );
  assert.equal(
    classifyTask({
      text: "请分析架构取舍",
      hasImages: false,
      chatMode: "ask",
      maxMode: false,
    }),
    "reasoning",
  );
});

test("selectAutoModel prefers lite/flash for simple", () => {
  const pick = selectAutoModel({
    text: "你好",
    hasImages: false,
    chatMode: "ask",
    maxMode: false,
    candidates,
    preferProviderId: "g1",
  });
  assert.ok(pick);
  assert.equal(pick.task, "simple");
  assert.ok(pick.modelId.includes("lite") || pick.modelId.includes("flash"));
  assert.equal(pick.modelId.includes("tts"), false);
});

test("selectAutoModel prefers pro/reasoning for hard tasks", () => {
  const pick = selectAutoModel({
    text: "请深入分析系统架构的 trade-off",
    hasImages: false,
    chatMode: "plan",
    maxMode: false,
    candidates,
    preferProviderId: "g1",
  });
  assert.ok(pick);
  assert.equal(pick.task, "reasoning");
  assert.equal(pick.modelId, "gemini-2.5-pro");
});

test("selectAutoModel vision keeps vision-capable flash/pro", () => {
  const pick = selectAutoModel({
    text: "看看这张图",
    hasImages: true,
    chatMode: "ask",
    maxMode: false,
    candidates,
  });
  assert.ok(pick);
  assert.equal(pick.task, "vision");
  assert.ok(
    pick.modelId === "gemini-2.5-flash" || pick.modelId === "gemini-2.5-pro",
  );
});
