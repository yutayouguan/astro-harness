import assert from "node:assert/strict";
import test from "node:test";
import type { ModelInfo } from "../../types";
import { toolLoadingStatus } from "./toolLoading.ts";

const model = (supports: boolean) =>
  ({ profile: { supports_search_tool: supports } }) as ModelInfo;

test("loading status distinguishes default, overrides, and unsupported fallback", () => {
  assert.equal(
    toolLoadingStatus("auto", "deferred", true, model(true)),
    "deferred",
  );
  assert.equal(
    toolLoadingStatus("auto", "direct", true, model(true)),
    "direct",
  );
  assert.equal(
    toolLoadingStatus("always", "deferred", true, model(false)),
    "direct",
  );
  assert.equal(
    toolLoadingStatus("on_demand", "direct", true, model(false)),
    "fallback",
  );
  assert.equal(
    toolLoadingStatus("auto", "deferred", true, model(false)),
    "fallback",
  );
});

test("loading status does not promise availability for unknown, disabled, or code-only models", () => {
  assert.equal(toolLoadingStatus("on_demand", "direct", true, null), "unknown");
  assert.equal(
    toolLoadingStatus("always", "deferred", false, model(true)),
    "disabled",
  );
  assert.equal(
    toolLoadingStatus("always", "direct", true, {
      ...model(true),
      tool_mode: "code_mode_only",
    }),
    "codeMode",
  );
});
