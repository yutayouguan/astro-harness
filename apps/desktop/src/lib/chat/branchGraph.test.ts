import assert from "node:assert/strict";
import { test } from "node:test";
import type { BranchGraphDto, BranchGraphNodeDto } from "../../types.ts";
import { buildBranchFlow } from "./branchGraph.ts";

function node(
  id: string,
  parentId: string | null,
  edgeKind: BranchGraphNodeDto["edgeKind"],
  kind: BranchGraphNodeDto["kind"] = "turn",
): BranchGraphNodeDto {
  return {
    id,
    kind,
    sessionId: kind === "agent" ? "agent-session" : "chat-session",
    parentId,
    edgeKind,
    title: id,
    preview: "",
    status: "completed",
    createdAt: null,
    sourceMessageId: null,
    turnIndex: null,
    model: null,
    agentPath: null,
    isCurrent: false,
    canFork: kind === "turn",
    isEphemeral: edgeKind === "side",
  };
}

function graph(nodes: BranchGraphNodeDto[]): BranchGraphDto {
  return {
    rootSessionId: "chat-session",
    currentSessionId: "chat-session",
    nodes,
    branchCount: 0,
    turnCount: nodes.filter((item) => item.kind === "turn").length,
    agentCount: nodes.filter((item) => item.kind === "agent").length,
    sideCount: nodes.filter((item) => item.isEphemeral).length,
  };
}

test("continuations remain on the same lane", () => {
  const flow = buildBranchFlow(graph([
    node("a", null, null),
    node("b", "a", "continuation"),
    node("c", "b", "continuation"),
  ]));
  assert.deepEqual(flow.nodes.map((item) => item.position.x), [0, 0, 0]);
  assert.deepEqual(flow.nodes.map((item) => item.position.y), [0, 168, 336]);
});

test("chat forks and agent spawns use separate sides and edge classes", () => {
  const flow = buildBranchFlow(graph([
    node("root", null, null),
    node("fork", "root", "fork"),
    node("agent", "root", "spawn", "agent"),
  ]));
  const positions = Object.fromEntries(flow.nodes.map((item) => [item.id, item.position.x]));
  assert.equal(positions.root, 0);
  assert.ok(positions.fork > 0);
  assert.ok(positions.agent < 0);
  assert.match(flow.edges.find((edge) => edge.target === "fork")?.className ?? "", /is-fork/);
  assert.match(flow.edges.find((edge) => edge.target === "agent")?.className ?? "", /is-spawn/);
});

test("ephemeral side conversations get a dedicated edge class", () => {
  const flow = buildBranchFlow(graph([
    node("root", null, null),
    node("side", "root", "side", "branchHead"),
  ]));
  assert.match(flow.edges[0]?.className ?? "", /is-side/);
  assert.ok((flow.nodes.find((item) => item.id === "side")?.position.x ?? 0) > 0);
});

test("cycles degrade to a finite layout", () => {
  const flow = buildBranchFlow(graph([
    node("a", "b", "continuation"),
    node("b", "a", "continuation"),
  ]));
  assert.equal(flow.nodes.length, 2);
  assert.ok(flow.nodes.every((item) => Number.isFinite(item.position.y)));
});
