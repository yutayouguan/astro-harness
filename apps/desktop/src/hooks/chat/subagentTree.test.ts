import assert from "node:assert/strict";
import test from "node:test";
import {
  classifyAgentThreadSessionEvent,
  createAgentTreeRootLifecycle,
  fromSnapshot,
  fromSnapshotWithBufferedEvents,
  markThreadRead,
  normalizeAgentThreadChanged,
  normalizeAgentThreadDetail,
  normalizeAgentTreeSnapshot,
  isAgentTreeGenerationCurrent,
  isAgentTreeRequestCurrent,
  reduceAgentThreadEvent,
  summarizeAgentActivity,
  type AgentThread,
  type AgentThreadChanged,
  type AgentTreeSnapshot,
} from "./subagentTree.ts";

function thread(
  canonicalPath: string,
  status: AgentThread["status"],
  overrides: Partial<AgentThread> = {},
): AgentThread {
  const segments = canonicalPath.split("/").filter(Boolean);
  const taskName = segments.at(-1) ?? "root";
  return {
    threadId: `${taskName}-id`,
    rootThreadId: "root-session",
    parentThreadId: canonicalPath === "/root" ? null : `${segments.at(-2) ?? "root"}-id`,
    canonicalPath,
    taskName,
    agentType: "default",
    sessionId: `${taskName}-session`,
    status,
    createdAt: "2026-08-19T00:00:00Z",
    updatedAt: "2026-08-19T00:00:01Z",
    ...overrides,
  };
}

function snapshot(threads: AgentThread[], activitySequence = 1): AgentTreeSnapshot {
  return { rootThreadId: "root-session", threads, activitySequence };
}

function changed(
  current: AgentThread,
  activitySequence: number,
  activityKind = "status_changed",
): AgentThreadChanged {
  return {
    ...current,
    activitySequence,
    activityKind,
    statusKind: current.status.kind,
    statusPayloadJson: JSON.stringify(current.status),
  };
}

test("builds a stable nested tree sorted by canonical path", () => {
  const root = thread("/root", { kind: "running" }, {
    threadId: "root-session",
    parentThreadId: null,
    sessionId: "root-session",
  });
  const research = thread("/root/research", { kind: "running" }, {
    threadId: "research-id",
    parentThreadId: "root-session",
  });
  const citations = thread("/root/research/citations", {
    kind: "completed",
    payload: { lastMessage: "done" },
  }, { parentThreadId: "research-id" });
  const analysis = thread("/root/analysis", { kind: "running" }, {
    parentThreadId: "root-session",
  });

  const state = fromSnapshot(snapshot([citations, research, analysis, root]));
  assert.deepEqual(
    state.roots.map((node) => node.thread.canonicalPath),
    ["/root"],
  );
  assert.deepEqual(
    state.roots[0]?.children.map((node) => node.thread.canonicalPath),
    ["/root/analysis", "/root/research"],
  );
  assert.equal(
    state.roots[0]?.children[1]?.children[0]?.thread.canonicalPath,
    "/root/research/citations",
  );
});

test("summarizes every visible subagent lifecycle state", () => {
  const state = fromSnapshot(
    snapshot([
      thread("/root/pending", { kind: "pending_init" }),
      thread("/root/running", { kind: "running" }),
      thread("/root/done", {
        kind: "completed",
        payload: { lastMessage: "done" },
      }),
      thread("/root/error", {
        kind: "errored",
        payload: { message: "failed" },
      }),
      thread("/root/interrupted", { kind: "interrupted" }),
    ]),
  );

  assert.deepEqual(summarizeAgentActivity(state.roots), {
    total: 5,
    pending: 1,
    running: 1,
    completed: 1,
    errored: 1,
    interrupted: 1,
    shutdown: 0,
  });
});

test("ignores duplicate and out-of-order activity sequences", () => {
  const worker = thread("/root/a", { kind: "running" });
  const initial = fromSnapshot(snapshot([worker], 10));
  const completed = reduceAgentThreadEvent(initial, changed({
    ...worker,
    status: { kind: "completed", payload: { lastMessage: "done" } },
  }, 11));
  assert.strictEqual(
    reduceAgentThreadEvent(completed, changed(worker, 10)),
    completed,
  );
  assert.strictEqual(
    reduceAgentThreadEvent(completed, changed(worker, 11)),
    completed,
  );
});

test("replays only newer buffered events from the active stream generation", () => {
  const worker = thread("/root/a", { kind: "running" });
  const initial = fromSnapshot(snapshot([worker], 8));
  const completed = changed({
    ...worker,
    status: { kind: "completed", payload: { lastMessage: "current" } },
  }, 11);
  const stale = changed(worker, 9);
  const otherGeneration = changed({
    ...worker,
    status: { kind: "errored", payload: { message: "wrong stream" } },
  }, 12);
  const next = fromSnapshotWithBufferedEvents(
    snapshot([worker], 10),
    initial,
    [
      { streamId: "current", changed: completed },
      { streamId: "current", changed: stale },
      { streamId: "old", changed: otherGeneration },
    ],
    "current",
  );
  assert.deepEqual(next.byPath["/root/a"]?.thread.status, {
    kind: "completed",
    payload: { lastMessage: "current" },
  });
  assert.equal(next.activitySequence, 11);
});

test("marks mailbox and final activity unread until the thread is opened", () => {
  const worker = thread("/root/a", { kind: "running" });
  const initial = fromSnapshot(snapshot([worker]));
  const mailbox = reduceAgentThreadEvent(initial, changed(worker, 2, "mailbox"));
  assert.equal(mailbox.byPath["/root/a"]?.unread, true);

  const read = markThreadRead(mailbox, "/root/a");
  assert.equal(read.byPath["/root/a"]?.unread, false);

  const completed = reduceAgentThreadEvent(read, changed({
    ...worker,
    status: { kind: "completed", payload: { lastMessage: "final" } },
  }, 3));
  assert.equal(completed.byPath["/root/a"]?.unread, true);
});

test("does not mark lifecycle noise unread", () => {
  const pending = thread("/root/a", { kind: "pending_init" });
  const initial = fromSnapshot(snapshot([pending]));
  const running = reduceAgentThreadEvent(initial, changed({
    ...pending,
    status: { kind: "running" },
  }, 2));
  assert.equal(running.byPath["/root/a"]?.unread, false);
});

test("keeps orphans stable and reparents them when the parent arrives", () => {
  const child = thread("/root/parent/child", { kind: "running" }, {
    threadId: "child-id",
    parentThreadId: "parent-id",
  });
  const initial = fromSnapshot(snapshot([child]));
  assert.equal(initial.roots[0]?.thread.threadId, "child-id");

  const parent = thread("/root/parent", { kind: "running" }, {
    threadId: "parent-id",
    parentThreadId: "root-session",
  });
  const next = reduceAgentThreadEvent(initial, changed(parent, 2, "spawned"));
  assert.equal(next.byPath["/root/parent"]?.children[0]?.thread.threadId, "child-id");
});

test("projects shutdown rows as archived and preserves unread state across snapshots", () => {
  const worker = thread("/root/a", { kind: "running" });
  const unread = reduceAgentThreadEvent(
    fromSnapshot(snapshot([worker], 3)),
    changed(worker, 4, "mailbox"),
  );
  const shutdown = thread("/root/a", { kind: "shutdown" });
  const refreshed = fromSnapshot(snapshot([shutdown], 5), unread);
  assert.equal(refreshed.byPath["/root/a"]?.archived, true);
  assert.equal(refreshed.byPath["/root/a"]?.unread, true);
});

test("marking an unknown path read is idempotent", () => {
  const state = fromSnapshot(snapshot([]));
  assert.strictEqual(markThreadRead(state, "/root/missing"), state);
});

test("normalizes the complete Tauri snapshot into camelCase V2 DTOs", () => {
  const normalized = normalizeAgentTreeSnapshot({
    root_thread_id: "root-session",
    activity_sequence: 9,
    threads: [{
      thread_id: "worker-id",
      root_thread_id: "root-session",
      parent_thread_id: "root-session",
      canonical_path: "/root/worker",
      task_name: "worker",
      agent_type: "reviewer",
      session_id: "worker-session",
      status: {
        kind: "completed",
        payload: { last_message: "finished" },
      },
      created_at: "created",
      updated_at: "updated",
    }],
  });
  assert.equal(normalized.activitySequence, 9);
  assert.deepEqual(normalized.threads[0], {
    threadId: "worker-id",
    rootThreadId: "root-session",
    parentThreadId: "root-session",
    canonicalPath: "/root/worker",
    taskName: "worker",
    agentType: "reviewer",
    sessionId: "worker-session",
    status: { kind: "completed", payload: { lastMessage: "finished" } },
    createdAt: "created",
    updatedAt: "updated",
  });
});

test("normalizes field 13 event status payload and empty root parent", () => {
  const event = normalizeAgentThreadChanged({
    activitySequence: 12,
    rootThreadId: "root-session",
    threadId: "root-session",
    parentThreadId: "",
    canonicalPath: "/root",
    taskName: "root",
    agentType: "default",
    sessionId: "root-session",
    statusKind: "shutdown",
    statusPayloadJson: "{\"kind\":\"shutdown\"}",
    activityKind: "edge_closed",
  });
  assert.equal(event.parentThreadId, null);
  assert.deepEqual(event.status, { kind: "shutdown" });
});

test("rejects a field 13 status discriminator that disagrees with its payload", () => {
  assert.throws(() => normalizeAgentThreadChanged({
    activitySequence: 12,
    rootThreadId: "root-session",
    threadId: "worker-id",
    parentThreadId: "root-session",
    canonicalPath: "/root/worker",
    taskName: "worker",
    agentType: "default",
    sessionId: "worker-session",
    statusKind: "running",
    statusPayloadJson: "{\"kind\":\"shutdown\"}",
    activityKind: "status_changed",
  }), /discriminator/);
});

test("normalizes the real SessionStore timeline including tool metadata", () => {
  const detail = normalizeAgentThreadDetail({
    thread: {
      thread_id: "worker-id",
      root_thread_id: "root-session",
      parent_thread_id: "root-session",
      canonical_path: "/root/worker",
      task_name: "worker",
      agent_type: "default",
      session_id: "worker-session",
      status: { kind: "running" },
      created_at: "created",
      updated_at: "updated",
    },
    messages: [{
      id: 4,
      session_id: "worker-session",
      role: "tool",
      content: "result",
      compressed_content: "short result",
      tool_call_id: "call-1",
      tool_calls: [{ name: "exec" }],
      tool_name: "exec",
      timestamp: 123.5,
      token_count: 8,
      finish_reason: null,
      reasoning: null,
      reasoning_content: null,
      reasoning_details: null,
      reasoning_items: null,
      message_items: null,
      media_json: null,
    }],
  });
  assert.equal(detail.messages[0]?.toolName, "exec");
  assert.equal(detail.messages[0]?.toolCallId, "call-1");
  assert.deepEqual(detail.messages[0]?.toolCalls, [{ name: "exec" }]);
});

test("ignores same-session local events before inspecting their empty stream id", () => {
  const result = classifyAgentThreadSessionEvent({
    sessionId: "root-session",
    streamId: "",
    memoryUpdated: { summary: "unrelated" },
  }, "root-session", "server-stream");
  assert.deepEqual(result, { kind: "ignore" });
});

test("classifies field 14 as refresh even when the stream id is unchanged", () => {
  const result = classifyAgentThreadSessionEvent({
    sessionId: "root-session",
    streamId: "server-stream",
    resyncRequired: { reason: "activity_gap" },
  }, "root-session", "server-stream");
  assert.equal(result.kind, "refresh");
  if (result.kind === "refresh") {
    assert.equal(result.streamChanged, false);
    assert.equal(result.nextStreamId, "server-stream");
  }
});

test("root generation and latest request reject stale async completions", () => {
  const ticket = { root: "root-a", generation: 3, request: 7 };
  assert.equal(isAgentTreeRequestCurrent(ticket, "root-a", 3, 7), true);
  assert.equal(isAgentTreeRequestCurrent(ticket, "root-b", 4, 7), false);
  assert.equal(isAgentTreeRequestCurrent(ticket, "root-a", 3, 8), false);
});

test("listener cleanup invalidates its captured root generation", () => {
  const listenerToken = { root: "root-a", generation: 5 };
  assert.equal(isAgentTreeGenerationCurrent(listenerToken, "root-a", 5), true);
  assert.equal(isAgentTreeGenerationCurrent(listenerToken, "root-a", 6), false);
});

test("aborted renders cannot change the committed Agent Tree root", () => {
  const lifecycle = createAgentTreeRootLifecycle();
  const rootA = lifecycle.commit("root-a");

  // Rendering B is intentionally represented by no lifecycle call: only a
  // committed layout effect may move the active root away from A.
  const speculativeRootB = { root: "root-b", generation: rootA.generation + 1 };
  assert.equal(lifecycle.isCurrent(rootA), true);
  assert.equal(lifecycle.isCurrent(speculativeRootB), false);

  lifecycle.invalidate(rootA);
  const rootB = lifecycle.commit("root-b");
  assert.equal(lifecycle.isCurrent(rootA), false);
  assert.equal(lifecycle.isCurrent(rootB), true);
});

test("StrictMode layout cleanup invalidates the first mount token", () => {
  const lifecycle = createAgentTreeRootLifecycle();
  const firstMount = lifecycle.commit("root-a");
  lifecycle.invalidate(firstMount);
  const strictRemount = lifecycle.commit("root-a");

  assert.equal(lifecycle.isCurrent(firstMount), false);
  assert.equal(lifecycle.isCurrent(strictRemount), true);
  assert.notEqual(strictRemount.generation, firstMount.generation);
});
