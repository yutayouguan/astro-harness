import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const providersPanel = await readFile(
  new URL("../../components/settings/ProvidersPanel.tsx", import.meta.url),
  "utf8",
);
const useProviders = await readFile(
  new URL("../../hooks/providers/useProviders.ts", import.meta.url),
  "utf8",
);

test("Agent provider selection exposes Responses-capable providers only", () => {
  assert.match(
    useProviders,
    /p\.enabled && p\.supports_responses_api === true/,
  );
  assert.match(providersPanel, /p\.supports_responses_api === true/);
});

test("provider settings no longer expose an Agent API mode selector", () => {
  assert.doesNotMatch(providersPanel, /chat_completions/);
  assert.doesNotMatch(providersPanel, /api_mode/);
});
