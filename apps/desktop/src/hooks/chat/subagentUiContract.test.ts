import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

function source(relative: string): string {
  return readFileSync(new URL(relative, import.meta.url), "utf8");
}

test("Agent Thread UI has no polling timers", () => {
  for (const file of [
    "./useSubagentThreads.ts",
    "../../components/chat/SubagentActivityBar.tsx",
    "../../components/chat/SubagentsPanel.tsx",
  ]) {
    const text = source(file);
    assert.doesNotMatch(text, /setInterval|clearInterval/, file);
  }
});

test("hook reacts to field 14 and stream generation changes", () => {
  const hook = source("./useSubagentThreads.ts");
  assert.match(hook, /classifyAgentThreadSessionEvent/);
  assert.match(hook, /classification\.kind === "refresh"/);
  assert.match(hook, /bufferedRef\.current\.push/);
  assert.match(hook, /isAgentTreeRequestCurrent/);
  assert.match(hook, /isAgentTreeGenerationCurrent/);
});

test("desktop commands use only canonical rootSessionId and target arguments", () => {
  const activity = source("../../components/chat/SubagentActivityBar.tsx");
  const panel = source("../../components/chat/SubagentsPanel.tsx");
  const hook = source("./useSubagentThreads.ts");
  const combined = `${activity}\n${panel}\n${hook}`;
  assert.match(combined, /rootSessionId/);
  assert.match(combined, /target:\s*thread\.canonicalPath/);
  assert.match(panel, /target:\s*canonicalPath/);
  assert.doesNotMatch(combined, /parentSessionId|threadId:/);
});

test("hierarchy styling uses depth and honors reduced motion", () => {
  const activity = source("../../components/chat/SubagentActivityBar.tsx");
  const panel = source("../../components/chat/SubagentsPanel.tsx");
  const css = source("../../styles/features/chat/subagents.css");
  assert.match(activity, /"--depth": depth/);
  assert.match(activity, /role="tree"/);
  assert.match(activity, /role="treeitem"/);
  assert.match(activity, /role="group"/);
  assert.match(panel, /!initialized/);
  assert.match(panel, /loading/);
  assert.match(css, /var\(--depth/);
  assert.match(css, /prefers-reduced-motion:\s*reduce/);
});
