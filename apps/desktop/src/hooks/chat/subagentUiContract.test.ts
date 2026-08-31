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
  const activity = source("../../components/chat/SubagentActivityBar.tsx");
  const panel = source("../../components/chat/SubagentsPanel.tsx");
  assert.match(hook, /classifyAgentThreadSessionEvent/);
  assert.match(hook, /classification\.kind === "refresh"/);
  assert.match(hook, /bufferedRef\.current\.push/);
  assert.match(hook, /isAgentTreeRequestCurrent/);
  assert.match(hook, /rootLifecycle\.isCurrent/);
  for (const component of [hook, activity, panel]) {
    assert.match(component, /useLayoutEffect/);
    assert.match(component, /createAgentTreeRootLifecycle/);
    assert.doesNotMatch(component, /activeRootRef\.current\s*=/);
  }
});

test("hook retries failed live-listener registration without leaking across roots", () => {
  const hook = source("./useSubagentThreads.ts");
  assert.match(hook, /const registerListener = \(\) =>/);
  assert.match(hook, /setTimeout\(registerListener, 1_000\)/);
  assert.match(hook, /clearTimeout\(retryTimer\)/);
  assert.match(hook, /if \(!isCurrent\(\)\) return/);
  assert.match(hook, /Listener-first startup\/recovery/);
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

test("subagent controls live in the pinned summary instead of the composer", () => {
  const app = source("../../App.tsx");
  const chatView = source("../../components/chat/ChatView.tsx");
  const rightPanel = source("../../components/chat/ChatRightPanel.tsx");
  const panel = source("../../components/chat/SubagentsPanel.tsx");
  const css = source("../../styles/features/chat/subagents.css");

  assert.match(app, /useSubagentThreads\(chat\.sessionId\)/);
  assert.match(app, /header-summary-badge/);
  assert.match(app, /subagentRoots=\{subagents\.roots\}/);
  assert.doesNotMatch(
    chatView,
    /SubagentActivityBar|SubagentsPanel|useSubagentThreads/,
  );
  assert.match(
    rightPanel,
    /type ChatRightTab = "summary" \| "context" \| "branches"/,
  );
  assert.match(rightPanel, /<ChatAgentInfo\s+variant="summary"/);
  assert.match(rightPanel, /<SubagentActivityBar[\s\S]+?showEmpty/);
  assert.match(rightPanel, /<SubagentsPanel\s+embedded/);
  assert.match(panel, /embedded\?: boolean/);
  assert.match(css, /\.subagents-panel\.is-embedded/);
});
