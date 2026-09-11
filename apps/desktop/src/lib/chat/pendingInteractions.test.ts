import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import {
  acceptInteractions,
  EMPTY_INTERACTIONS,
  inlineInteraction,
  simpleSchema,
  redactInteractionDisplay,
  taskRows,
  type PendingInteraction,
  type InteractionTask,
} from "./pendingInteractions.ts";
const request: PendingInteraction = {
  key: "s/t/r",
  sessionId: "s",
  turnId: "t",
  requestId: "r",
  toolCallId: "call",
  kind: "question",
  message: "原始问题",
  operations: [],
  responseSchema: {
    type: "object",
    properties: { name: { type: "string" } },
    required: ["name"],
  },
  actions: [],
  expiresAt: "",
  serverName: null,
  generation: null,
};
test("interaction snapshots reject out-of-order updates but accept backend restarts", () => {
  const current = {
    ...EMPTY_INTERACTIONS,
    connected: true,
    snapshot: {
      ...EMPTY_INTERACTIONS.snapshot,
      epoch: "one",
      revision: 8,
      requests: [request],
    },
  };
  assert.equal(
    acceptInteractions(current, {
      ...current,
      snapshot: { ...current.snapshot, revision: 7, requests: [] },
    }),
    current,
  );
  assert.equal(
    acceptInteractions(current, {
      ...current,
      snapshot: {
        ...current.snapshot,
        epoch: "two",
        revision: 0,
        requests: [],
      },
    }).snapshot.requests.length,
    0,
  );
});
test("only advertised approval options and supported original forms are interactive", () => {
  assert.equal(inlineInteraction(request), true);
  assert.equal(inlineInteraction({ ...request, kind: "approval" }), false);
  assert.equal(inlineInteraction({ ...request, kind: "external" }), false);
  assert.equal(
    simpleSchema({
      ...request,
      responseSchema: {
        type: "object",
        properties: { nested: { type: "object" } },
      },
    }),
    null,
  );
});
test("operation previews redact common inline credentials", () => {
  const display = redactInteractionDisplay(
    "OPENAI_API_KEY=sk-abcdefghijklmnop Bearer abcdef-secret password='test-secret'",
  );
  assert.ok(!display.includes("abcdefghijklmnop"));
  assert.ok(!display.includes("abcdef-secret"));
  assert.ok(!display.includes("test-secret"));
});
test("a late reply from a retired backend cannot resurrect its requests", () => {
  const first = {
    ...EMPTY_INTERACTIONS,
    connected: true,
    snapshot: {
      ...EMPTY_INTERACTIONS.snapshot,
      epoch: "first",
      revision: 9,
      requests: [request],
    },
  };
  const second = acceptInteractions(first, {
    ...first,
    snapshot: { ...first.snapshot, epoch: "second", revision: 1, requests: [] },
  });
  assert.equal(acceptInteractions(second, first), second);
  assert.equal(acceptInteractions(second, EMPTY_INTERACTIONS), second);
});
test("nested and orphan task relationships keep every task reachable", () => {
  const task = (
    sessionId: string,
    parentSessionId: string | null,
  ): InteractionTask => ({
    sessionId,
    parentSessionId,
    turnId: "t",
    title: sessionId,
    project: "",
    status: "running",
  });
  assert.deepEqual(
    taskRows([
      task("root", null),
      task("child", "root"),
      task("grandchild", "child"),
    ]).map((r) => r.depth),
    [0, 1, 2],
  );
  assert.equal(
    taskRows([task("a", "b"), task("b", "a"), task("orphan", "missing")])
      .length,
    3,
  );
});

test("legacy cards cannot bulk-resolve and task navigation retains detached drafts", () => {
  const source = readFileSync(
    new URL("../../hooks/chat/useChatSession.ts", import.meta.url),
    "utf8",
  );
  assert.match(source, /await respondInteraction\(\s*live/);
  assert.ok(source.includes("interrupts.length !== 1"));
  assert.ok(!source.includes("sessionPendingInterrupts.map((p) => ({"));
  assert.ok(source.includes('petNavigationDrafts.current.get("new")'));
  assert.ok(
    source.includes("petNavigationDrafts.current.delete(targetSessionId)"),
  );
  assert.ok(source.includes("if (!preserveCurrent)"));
});

test("the selected popup request is synchronized with native queue ownership", () => {
  const source = readFileSync(
    new URL("../../components/desktop-pet/PetTaskSurface.tsx", import.meta.url),
    "utf8",
  );
  assert.ok(source.includes('invoke("open_pet_tasks", { requestKey: key })'));
  assert.ok(source.includes("selectRequest(r.key)"));
});
