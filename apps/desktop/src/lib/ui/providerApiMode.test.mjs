import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const providersPanel = await readFile(
  new URL("../../components/settings/ProvidersPanel.tsx", import.meta.url),
  "utf8",
);

test("Responses is the primary API mode and Chat Completions remains explicit compatibility", () => {
  const responses = providersPanel.indexOf(
    '{ value: "responses", label: "Responses API" }',
  );
  const chat = providersPanel.indexOf(
    '{ value: "chat_completions", label: "Chat Completions (兼容)" }',
  );

  assert.ok(responses >= 0, "missing Responses API option");
  assert.ok(chat > responses, "Chat Completions compatibility should be the second option");
  assert.match(
    providersPanel,
    /d \? \{ \.\.\.d, api_mode: v \} : d/,
    "the selected compatibility mode should be persisted explicitly",
  );
});
