import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { parseHitlRunFinished } from "./hitlRunFinished.ts";
import {
  acceptInteractions,
  EMPTY_INTERACTIONS,
  inlineInteraction,
  interactionIdentity,
  interactionMatchesInterrupt,
  interruptIdentity,
  legacyInteractionResponse,
  simpleSchema,
  redactInteractionDisplay,
  taskRows,
  type PendingInteraction,
  type InteractionTask,
} from "./pendingInteractions.ts";
test("native task glass keeps the UA canvas transparent and sizes through scoped IPC", () => {
  const main = readFileSync(new URL("../../main.tsx", import.meta.url), "utf8");
  const native = readFileSync(
    new URL("../../../src-tauri/src/commands/ui/pet_tasks.rs", import.meta.url),
    "utf8",
  );
  assert.match(
    main,
    /style\.colorScheme\s*=\s*isDesktopPetWindow\s*\|\|\s*isPetTaskWindow/,
  );
  assert.ok(native.includes(".transparent(true)"));
  assert.ok(native.includes("NSVisualEffectMaterial::Popover"));
  const sizing = native.slice(
    native.indexOf("pub fn resize_pet_task_content"),
    native.indexOf("/// Physical-coordinate placement"),
  );
  assert.ok(sizing.includes("window.label() != POPUP"));
});

test("native task close uses snooze before the generic window hide path", () => {
  const shell = readFileSync(
    new URL("../../../src-tauri/src/lib.rs", import.meta.url),
    "utf8",
  );
  const handler = shell.slice(
    shell.indexOf("WindowEvent::CloseRequested"),
    shell.indexOf(".invoke_handler"),
  );
  assert.ok(handler.indexOf("close_from_native") >= 0);
  assert.ok(
    handler.indexOf("close_from_native") < handler.indexOf("window.hide()"),
  );
});

const request: PendingInteraction = {
  key: "s/t/r",
  sessionId: "s",
  turnId: "t",
  requestId: "r",
  toolCallId: "call",
  kind: "question",
  reason: "input_required",
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

test("late desktop replies cannot reset selection without a backend revision change", () => {
  const current = {
    ...EMPTY_INTERACTIONS,
    uiRevision: 9,
    connected: true,
    selected: "editing",
    snapshot: { ...EMPTY_INTERACTIONS.snapshot, epoch: "backend", revision: 3 },
  };
  const oldReply = { ...current, uiRevision: 8, selected: null };
  assert.equal(acceptInteractions(current, oldReply), current);
  assert.equal(
    acceptInteractions(current, {
      ...current,
      uiRevision: 10,
      selected: "next",
    }).selected,
    "next",
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

test("quoted JSON and space-containing credentials are fully redacted", () => {
  const display = redactInteractionDisplay(
    'curl -d \'{"api_key":"json-secret", "password":"space secret"}\' PASSWORD="shell secret"',
  );
  for (const secret of ["json-secret", "space secret", "shell secret"])
    assert.ok(!display.includes(secret));
});

test("MCP legacy actions keep decline/cancel and schema-typed answers", () => {
  const mcp = {
    ...request,
    serverName: "server",
    toolCallId: "mcp-elicitation:server:r",
    responseSchema: {
      type: "object",
      properties: { count: { type: "integer" }, enabled: { type: "boolean" } },
    },
  };
  assert.deepEqual(
    legacyInteractionResponse(mcp, "choose", {
      answers: { count: "3", enabled: "true" },
      value: "summary",
    }),
    {
      action: "submit",
      payload: { count: 3, enabled: true },
      persistent: false,
    },
  );
  assert.deepEqual(
    legacyInteractionResponse(mcp, "deny", { answers: { count: "3" } }),
    { action: "decline", payload: {}, persistent: false },
  );
  assert.equal(legacyInteractionResponse(mcp, "cancel", {}).action, "cancel");
  assert.equal(legacyInteractionResponse(mcp, "decline", {}).action, "decline");
});

test("MCP reconciliation uses server-qualified UI aliases, not colliding raw IDs", () => {
  const mcp = {
    ...request,
    serverName: "one",
    toolCallId: "mcp-elicitation:one:r",
  };
  const [one, two] = parseHitlRunFinished(
    JSON.stringify([
      { id: "r", reason: "elicitation", tool_call_id: "mcp-elicitation:one:r" },
      { id: "r", reason: "elicitation", tool_call_id: "mcp-elicitation:two:r" },
    ]),
    "assistant",
  ).interrupts;
  assert.equal(one.id, two.id); // Actual transport retains the server's raw ID.
  assert.equal(interactionIdentity(mcp), interruptIdentity(one));
  assert.equal(interactionMatchesInterrupt(mcp, "s", one), true);
  assert.equal(interactionMatchesInterrupt(mcp, "s", two), false);
  assert.equal(interactionMatchesInterrupt(mcp, "other", one), false);
  assert.equal(
    interactionMatchesInterrupt(mcp, "s", { id: "r", reason: "confirmation" }),
    false,
  );
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
  const navigation = source.slice(
    source.indexOf("const openSessionFromFilespace ="),
  );
  assert.match(
    navigation,
    /\+\+streamGenRef.current;\s*unlistenRef.current\?\.\(\);\s*unlistenRef.current = null;\s*clearStreamBuffers\(\)/,
  );
  assert.ok(source.includes("completedBeforeRead"));
});

test("the selected popup request is synchronized with native queue ownership", () => {
  const source = readFileSync(
    new URL("../../components/desktop-pet/PetTaskSurface.tsx", import.meta.url),
    "utf8",
  );
  assert.ok(source.includes('invoke("open_pet_tasks", { requestKey: key })'));
  assert.ok(source.includes("selectRequest(r.key)"));
});
