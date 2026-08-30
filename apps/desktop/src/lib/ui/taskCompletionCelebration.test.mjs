import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const root = new URL("../../", import.meta.url);

async function source(path) {
  return readFile(new URL(path, root), "utf8");
}

test("successful primary and parallel tasks request a celebration", async () => {
  const [send, parallel, session, completion] = await Promise.all([
    source("hooks/chat/useSend.ts"),
    source("hooks/chat/useParallelTasks.ts"),
    source("hooks/chat/useChatSession.ts"),
    source("lib/chat/taskCompletion.ts"),
  ]);

  assert.match(send, /resolveTaskCompletion\([\s\S]*completion\.celebrate/);
  assert.match(parallel, /resolveParallelTaskCompletion\([\s\S]*completion\.celebrate/);
  assert.match(send, /completionSettled \|\| terminalOutcome === "hitl_waiting"/);
  assert.match(parallel, /if \(completionSettled\) return/);
  assert.match(completion, /outcome === "success" && !failed/);
  assert.match(completion, /outcome === "interrupt"/);
  assert.match(completion, /outcome === "hitl_waiting"/);
  assert.match(session, /setCompletionCelebrationId\(\(current\) => current \+ 1\)/);
  assert.match(session, /onTurnSucceeded: celebrateTaskCompletion/);
  assert.match(session, /onTaskSucceeded: celebrateTaskCompletion/);
});

test("celebration is a non-interactive content overlay with reduced-motion fallback", async () => {
  const [component, view, styles] = await Promise.all([
    source("components/chat/TaskCompletionCelebration.tsx"),
    source("components/chat/ChatView.tsx"),
    source("styles/features/chat/completion-celebration.css"),
  ]);

  assert.match(component, /requestAnimationFrame/);
  assert.match(component, /prefers-reduced-motion: reduce/);
  assert.match(component, /previousTriggerRef = useRef\(trigger\)/);
  assert.match(component, /shouldStartCompletionCelebration\(previousTrigger, trigger\)/);
  assert.match(component, /const releaseCanvasBackingStore = \(\) =>/);
  assert.match(component, /canvas\.width = 1/);
  assert.match(component, /setTimeout\(releaseCanvasBackingStore, 560\)/);
  assert.match(component, /aria-hidden="true"/);
  assert.match(view, /<TaskCompletionCelebration trigger=\{completionCelebrationId\}/);
  assert.match(styles, /position: absolute/);
  assert.match(styles, /inset: 0/);
  assert.match(styles, /pointer-events: none/);
});
