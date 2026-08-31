export type AgentThreadStatus =
  | { kind: "pending_init" }
  | { kind: "running" }
  | { kind: "interrupted" }
  | { kind: "completed"; payload: { lastMessage: string } }
  | { kind: "errored"; payload: { message: string } }
  | { kind: "shutdown" };

export type AgentThread = {
  threadId: string;
  rootThreadId: string;
  parentThreadId: string | null;
  canonicalPath: string;
  taskName: string;
  agentType: string;
  sessionId: string;
  status: AgentThreadStatus;
  createdAt: string;
  updatedAt: string;
};

export type AgentTreeSnapshot = {
  rootThreadId: string;
  threads: AgentThread[];
  activitySequence: number;
};

export type AgentThreadMessage = {
  id: number;
  sessionId: string;
  role: string;
  content: string | null;
  compressedContent: string | null;
  toolCallId: string | null;
  toolCalls: unknown | null;
  toolName: string | null;
  timestamp: number;
  tokenCount: number | null;
  finishReason: string | null;
  reasoning: string | null;
  reasoningDetails: unknown | null;
  mediaJson: string | null;
};

export type AgentThreadDetail = {
  thread: AgentThread;
  messages: AgentThreadMessage[];
};

export type AgentThreadChanged = Omit<
  AgentThread,
  "createdAt" | "updatedAt"
> & {
  activitySequence: number;
  activityKind: string;
  statusKind: AgentThreadStatus["kind"];
  statusPayloadJson: string;
};

export type BufferedAgentThreadChanged = {
  streamId: string;
  changed: AgentThreadChanged;
};

export type AgentThreadSessionEventPayload = {
  sessionId?: string | null;
  streamId?: string;
  agentThreadChanged?: unknown | null;
  resyncRequired?: { reason?: string } | null;
  [key: string]: unknown;
};

export type AgentThreadSessionEventClassification =
  | { kind: "ignore" }
  | {
      kind: "delta";
      changed: AgentThreadChanged;
      nextStreamId: string;
    }
  | {
      kind: "refresh";
      changed: AgentThreadChanged | null;
      nextStreamId: string;
      streamChanged: boolean;
    };

export type AgentTreeRequestTicket = {
  root: string;
  generation: number;
  request: number;
};

export type AgentTreeGenerationToken = Omit<AgentTreeRequestTicket, "request">;

export type AgentTreeRootLifecycle = {
  current: () => AgentTreeGenerationToken;
  commit: (root: string) => AgentTreeGenerationToken;
  invalidate: (token: AgentTreeGenerationToken) => void;
  isCurrent: (token: AgentTreeGenerationToken) => boolean;
};

export type AgentTreeNode = {
  thread: AgentThread;
  children: AgentTreeNode[];
  unread: boolean;
  archived: boolean;
};

export type AgentTreeState = {
  rootThreadId: string;
  activitySequence: number;
  byPath: Record<string, AgentTreeNode>;
  roots: AgentTreeNode[];
  threads: AgentThread[];
};

export const EMPTY_AGENT_TREE: AgentTreeState = {
  rootThreadId: "",
  activitySequence: 0,
  byPath: {},
  roots: [],
  threads: [],
};

type JsonRecord = Record<string, unknown>;

function record(value: unknown, label: string): JsonRecord {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`invalid ${label}`);
  }
  return value as JsonRecord;
}

function stringField(value: unknown, label: string): string {
  if (typeof value !== "string") throw new Error(`invalid ${label}`);
  return value;
}

function numberField(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new Error(`invalid ${label}`);
  }
  return value;
}

function finiteNumberField(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(`invalid ${label}`);
  }
  return value;
}

function nullableString(value: unknown, label: string): string | null {
  if (value == null) return null;
  return stringField(value, label);
}

function nullableInteger(value: unknown, label: string): number | null {
  if (value == null) return null;
  if (typeof value !== "number" || !Number.isSafeInteger(value)) {
    throw new Error(`invalid ${label}`);
  }
  return value;
}

export function normalizeAgentThreadStatus(value: unknown): AgentThreadStatus {
  const status = record(value, "agent thread status");
  const kind = stringField(status.kind, "agent thread status kind");
  if (
    kind === "pending_init" ||
    kind === "running" ||
    kind === "interrupted" ||
    kind === "shutdown"
  ) {
    return { kind };
  }
  const payload = record(status.payload, `${kind} payload`);
  if (kind === "completed") {
    return {
      kind,
      payload: {
        lastMessage: stringField(
          payload.lastMessage ?? payload.last_message,
          "completed last message",
        ),
      },
    };
  }
  if (kind === "errored") {
    return {
      kind,
      payload: { message: stringField(payload.message, "errored message") },
    };
  }
  throw new Error(`unknown agent thread status ${kind}`);
}

export function normalizeAgentThread(value: unknown): AgentThread {
  const item = record(value, "agent thread");
  const parent = item.parentThreadId ?? item.parent_thread_id;
  return {
    threadId: stringField(item.threadId ?? item.thread_id, "thread id"),
    rootThreadId: stringField(
      item.rootThreadId ?? item.root_thread_id,
      "root thread id",
    ),
    parentThreadId:
      parent == null ? null : stringField(parent, "parent thread id"),
    canonicalPath: stringField(
      item.canonicalPath ?? item.canonical_path,
      "canonical path",
    ),
    taskName: stringField(item.taskName ?? item.task_name, "task name"),
    agentType: stringField(item.agentType ?? item.agent_type, "agent type"),
    sessionId: stringField(item.sessionId ?? item.session_id, "session id"),
    status: normalizeAgentThreadStatus(item.status),
    createdAt: stringField(item.createdAt ?? item.created_at, "created at"),
    updatedAt: stringField(item.updatedAt ?? item.updated_at, "updated at"),
  };
}

/** Convert the current Rust/Tauri wire casing into the UI's camelCase V2 DTO. */
export function normalizeAgentTreeSnapshot(value: unknown): AgentTreeSnapshot {
  const snapshot = record(value, "agent tree snapshot");
  const threads = snapshot.threads;
  if (!Array.isArray(threads)) throw new Error("invalid agent tree threads");
  return {
    rootThreadId: stringField(
      snapshot.rootThreadId ?? snapshot.root_thread_id,
      "snapshot root thread id",
    ),
    threads: threads.map(normalizeAgentThread),
    activitySequence: numberField(
      snapshot.activitySequence ?? snapshot.activity_sequence,
      "snapshot activity sequence",
    ),
  };
}

export function normalizeAgentThreadMessage(
  value: unknown,
): AgentThreadMessage {
  const message = record(value, "agent thread message");
  return {
    id: numberField(message.id, "message id"),
    sessionId: stringField(
      message.sessionId ?? message.session_id,
      "message session id",
    ),
    role: stringField(message.role, "message role"),
    content: nullableString(message.content, "message content"),
    compressedContent: nullableString(
      message.compressedContent ?? message.compressed_content,
      "message compressed content",
    ),
    toolCallId: nullableString(
      message.toolCallId ?? message.tool_call_id,
      "tool call id",
    ),
    toolCalls: message.toolCalls ?? message.tool_calls ?? null,
    toolName: nullableString(
      message.toolName ?? message.tool_name,
      "tool name",
    ),
    timestamp: finiteNumberField(message.timestamp, "message timestamp"),
    tokenCount: nullableInteger(
      message.tokenCount ?? message.token_count,
      "token count",
    ),
    finishReason: nullableString(
      message.finishReason ?? message.finish_reason,
      "finish reason",
    ),
    reasoning: nullableString(message.reasoning, "reasoning"),
    reasoningDetails:
      message.reasoningDetails ?? message.reasoning_details ?? null,
    mediaJson: nullableString(
      message.mediaJson ?? message.media_json,
      "media json",
    ),
  };
}

export function normalizeAgentThreadDetail(value: unknown): AgentThreadDetail {
  const detail = record(value, "agent thread detail");
  if (!Array.isArray(detail.messages))
    throw new Error("invalid agent thread messages");
  return {
    thread: normalizeAgentThread(detail.thread),
    messages: detail.messages.map(normalizeAgentThreadMessage),
  };
}

export function normalizeAgentThreadChanged(
  value: unknown,
): AgentThreadChanged {
  const event = record(value, "agent thread changed event");
  const statusPayload = stringField(
    event.statusPayloadJson,
    "status payload json",
  );
  const status = normalizeAgentThreadStatus(JSON.parse(statusPayload));
  const statusKind = stringField(event.statusKind, "event status kind");
  if (status.kind !== statusKind) {
    throw new Error(
      "agent thread status discriminator does not match its payload",
    );
  }
  return {
    threadId: stringField(event.threadId, "event thread id"),
    rootThreadId: stringField(event.rootThreadId, "event root thread id"),
    parentThreadId:
      event.parentThreadId === "" || event.parentThreadId == null
        ? null
        : stringField(event.parentThreadId, "event parent thread id"),
    canonicalPath: stringField(event.canonicalPath, "event canonical path"),
    taskName: stringField(event.taskName, "event task name"),
    agentType: stringField(event.agentType, "event agent type"),
    sessionId: stringField(event.sessionId, "event session id"),
    status,
    statusKind: status.kind,
    statusPayloadJson: statusPayload,
    activitySequence: numberField(
      event.activitySequence,
      "event activity sequence",
    ),
    activityKind: stringField(event.activityKind, "event activity kind"),
  };
}

export function classifyAgentThreadSessionEvent(
  payload: AgentThreadSessionEventPayload,
  root: string,
  desiredStreamId: string | null,
): AgentThreadSessionEventClassification {
  const hasChanged = payload.agentThreadChanged != null;
  const hasResync = payload.resyncRequired != null;
  // Memory/title/local-only events share the same channel and often carry an
  // empty stream id. They must not participate in Agent Tree generations.
  if (!hasChanged && !hasResync) return { kind: "ignore" };
  if (payload.sessionId?.trim() !== root) return { kind: "ignore" };

  const nextStreamId = payload.streamId ?? "";
  const streamChanged =
    desiredStreamId != null && desiredStreamId !== nextStreamId;
  const changed = hasChanged
    ? normalizeAgentThreadChanged(payload.agentThreadChanged)
    : null;
  if (changed && changed.rootThreadId !== root) return { kind: "ignore" };
  if (hasResync || streamChanged) {
    return { kind: "refresh", changed, nextStreamId, streamChanged };
  }
  if (!changed) return { kind: "ignore" };
  return { kind: "delta", changed, nextStreamId };
}

export function isAgentTreeRequestCurrent(
  ticket: AgentTreeRequestTicket,
  activeRoot: string,
  activeGeneration: number,
  latestRequest: number,
): boolean {
  return (
    ticket.root === activeRoot &&
    ticket.generation === activeGeneration &&
    ticket.request === latestRequest
  );
}

export function isAgentTreeGenerationCurrent(
  token: AgentTreeGenerationToken,
  activeRoot: string,
  activeGeneration: number,
): boolean {
  return token.root === activeRoot && token.generation === activeGeneration;
}

export function createAgentTreeRootLifecycle(): AgentTreeRootLifecycle {
  let root = "";
  let generation = 0;
  const current = (): AgentTreeGenerationToken => ({ root, generation });
  return {
    current,
    commit(nextRoot) {
      if (root !== nextRoot) {
        root = nextRoot;
        generation += 1;
      }
      return current();
    },
    invalidate(token) {
      if (isAgentTreeGenerationCurrent(token, root, generation)) {
        generation += 1;
      }
    },
    isCurrent(token) {
      return isAgentTreeGenerationCurrent(token, root, generation);
    },
  };
}

function compareNodes(left: AgentTreeNode, right: AgentTreeNode): number {
  return compareCanonicalPath(
    left.thread.canonicalPath,
    right.thread.canonicalPath,
  );
}

function compareCanonicalPath(left: string, right: string): number {
  return left === right ? 0 : left < right ? -1 : 1;
}

function projectTree(
  rootThreadId: string,
  activitySequence: number,
  threads: AgentThread[],
  unreadByPath: Readonly<Record<string, boolean>>,
): AgentTreeState {
  const ordered = [...threads].sort((left, right) =>
    compareCanonicalPath(left.canonicalPath, right.canonicalPath),
  );
  const byId = new Map<string, AgentTreeNode>();
  const byPath: Record<string, AgentTreeNode> = {};

  for (const thread of ordered) {
    const node: AgentTreeNode = {
      thread,
      children: [],
      unread: unreadByPath[thread.canonicalPath] ?? false,
      archived: thread.status.kind === "shutdown",
    };
    byId.set(thread.threadId, node);
    byPath[thread.canonicalPath] = node;
  }

  const roots: AgentTreeNode[] = [];
  for (const thread of ordered) {
    const node = byId.get(thread.threadId);
    if (!node) continue;
    const parent = thread.parentThreadId
      ? byId.get(thread.parentThreadId)
      : undefined;
    // A missing or self-referential parent is a stable orphan root until a
    // later snapshot/event supplies the edge.
    if (parent && parent !== node) parent.children.push(node);
    else roots.push(node);
  }

  for (const node of Object.values(byPath)) node.children.sort(compareNodes);
  roots.sort(compareNodes);
  return { rootThreadId, activitySequence, byPath, roots, threads: ordered };
}

export function fromSnapshot(
  snapshot: AgentTreeSnapshot,
  previous: AgentTreeState = EMPTY_AGENT_TREE,
): AgentTreeState {
  const unreadByPath = Object.fromEntries(
    Object.entries(previous.byPath).map(([path, node]) => [path, node.unread]),
  );
  return projectTree(
    snapshot.rootThreadId,
    snapshot.activitySequence,
    snapshot.threads,
    unreadByPath,
  );
}

export function fromSnapshotWithBufferedEvents(
  snapshot: AgentTreeSnapshot,
  previous: AgentTreeState,
  buffered: readonly BufferedAgentThreadChanged[],
  streamId: string | null,
): AgentTreeState {
  let next = fromSnapshot(snapshot, previous);
  const currentGeneration = buffered
    .filter((item) => streamId == null || item.streamId === streamId)
    .sort(
      (left, right) =>
        left.changed.activitySequence - right.changed.activitySequence,
    );
  for (const item of currentGeneration) {
    next = reduceAgentThreadEvent(next, item.changed);
  }
  return next;
}

function isUnreadActivity(event: AgentThreadChanged): boolean {
  if (event.activityKind === "mailbox") return true;
  return (
    event.status.kind === "completed" ||
    event.status.kind === "errored" ||
    event.status.kind === "interrupted"
  );
}

export function reduceAgentThreadEvent(
  state: AgentTreeState,
  event: AgentThreadChanged,
): AgentTreeState {
  if (event.rootThreadId !== state.rootThreadId) return state;
  if (event.activitySequence <= state.activitySequence) return state;

  const current = state.byPath[event.canonicalPath];
  const nextThread: AgentThread = {
    threadId: event.threadId,
    rootThreadId: event.rootThreadId,
    parentThreadId: event.parentThreadId,
    canonicalPath: event.canonicalPath,
    taskName: event.taskName,
    agentType: event.agentType,
    sessionId: event.sessionId,
    status: event.status,
    createdAt: current?.thread.createdAt ?? "",
    updatedAt: current?.thread.updatedAt ?? "",
  };
  const threads = state.threads.filter(
    (thread) =>
      thread.threadId !== event.threadId &&
      thread.canonicalPath !== event.canonicalPath,
  );
  threads.push(nextThread);

  const unreadByPath = Object.fromEntries(
    Object.entries(state.byPath).map(([path, node]) => [path, node.unread]),
  );
  if (isUnreadActivity(event)) unreadByPath[event.canonicalPath] = true;
  return projectTree(
    state.rootThreadId,
    event.activitySequence,
    threads,
    unreadByPath,
  );
}

export function markThreadRead(
  state: AgentTreeState,
  path: string,
): AgentTreeState {
  const node = state.byPath[path];
  if (!node?.unread) return state;
  const unreadByPath = Object.fromEntries(
    Object.entries(state.byPath).map(([currentPath, current]) => [
      currentPath,
      currentPath === path ? false : current.unread,
    ]),
  );
  return projectTree(
    state.rootThreadId,
    state.activitySequence,
    state.threads,
    unreadByPath,
  );
}

export function flattenAgentTree(
  nodes: readonly AgentTreeNode[],
): AgentTreeNode[] {
  return nodes.flatMap((node) => [node, ...flattenAgentTree(node.children)]);
}

export type AgentActivitySummary = {
  total: number;
  pending: number;
  running: number;
  completed: number;
  errored: number;
  interrupted: number;
  shutdown: number;
};

export function summarizeAgentActivity(
  nodes: readonly AgentTreeNode[],
): AgentActivitySummary {
  const summary: AgentActivitySummary = {
    total: 0,
    pending: 0,
    running: 0,
    completed: 0,
    errored: 0,
    interrupted: 0,
    shutdown: 0,
  };
  for (const node of flattenAgentTree(nodes)) {
    summary.total += 1;
    switch (node.thread.status.kind) {
      case "pending_init":
        summary.pending += 1;
        break;
      case "running":
        summary.running += 1;
        break;
      case "completed":
        summary.completed += 1;
        break;
      case "errored":
        summary.errored += 1;
        break;
      case "interrupted":
        summary.interrupted += 1;
        break;
      case "shutdown":
        summary.shutdown += 1;
        break;
    }
  }
  return summary;
}
