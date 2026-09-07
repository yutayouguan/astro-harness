import assert from "node:assert/strict";
import test from "node:test";

import {
  mediaFromStreamEvent,
  toolActivityKind,
  usageFromStreamEvent,
} from "./chatStreamEvent.ts";

test("usage events preserve reported flags and fallback uncached input", () => {
  assert.deepEqual(
    usageFromStreamEvent({
      type: "usage",
      prompt_tokens: 100,
      completion_tokens: 25,
      total_tokens: 125,
      cache_read_tokens: 40,
      cache_read_reported: true,
    }),
    {
      promptTokens: 100,
      uncachedInputTokens: 100,
      completionTokens: 25,
      totalTokens: 125,
      providerTotalTokens: undefined,
      cacheReadTokens: 40,
      cacheWriteTokens: 0,
      reasoningTokens: 0,
      requestCount: 0,
      cacheReadReported: true,
      cacheWriteReported: false,
      reasoningReported: false,
    },
  );
});

test("tool activity kinds follow model-visible namespaces", () => {
  assert.equal(toolActivityKind("mcp_files_read"), "mcp");
  assert.equal(toolActivityKind("skill_load"), "skill");
  assert.equal(toolActivityKind("plugin_skill_sync"), "skill");
  assert.equal(toolActivityKind("hook_pre_tool"), "hook");
  assert.equal(toolActivityKind("exec_command"), "tool");
});

test("file media is inferred from safe extensions", () => {
  assert.deepEqual(
    mediaFromStreamEvent([
      { kind: "file", ref_value: "generated/preview.webp" },
      { kind: "file", ref_value: "generated/report.html" },
      { kind: "file", ref_value: "generated/main.rs" },
      { kind: "file", ref_value: "generated/unknown.bin" },
    ]),
    [
      { kind: "image", path: "generated/preview.webp" },
      { kind: "html", path: "generated/report.html" },
      { kind: "code", path: "generated/main.rs" },
    ],
  );
});
