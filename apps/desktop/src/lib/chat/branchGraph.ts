import type { Edge, Node } from "@xyflow/react";
import type { BranchGraphDto, BranchGraphNodeDto } from "../../types";

export type BranchNodeData = Record<string, unknown> & {
  node: BranchGraphNodeDto;
};

export type BranchFlowNode = Node<BranchNodeData, "branchNode">;

export type BranchFlowModel = {
  nodes: BranchFlowNode[];
  edges: Edge[];
};

const X_GAP = 276;
const Y_GAP = 168;

function edgeClass(kind: BranchGraphNodeDto["edgeKind"]): string {
  if (kind === "fork") return "branch-edge is-fork";
  if (kind === "spawn") return "branch-edge is-spawn";
  return "branch-edge is-continuation";
}

/**
 * 把后端已消除共享前缀重复的谱系变成稳定的纵向轨道。
 * continuation 留在父轨，fork 向右展开，spawn 向左展开。
 */
export function buildBranchFlow(graph: BranchGraphDto): BranchFlowModel {
  const source = new Map(graph.nodes.map((node) => [node.id, node]));
  const depth = new Map<string, number>();
  const lane = new Map<string, number>();
  let nextForkLane = 1;
  let nextAgentLane = -1;

  const place = (id: string, visiting = new Set<string>()): void => {
    if (depth.has(id)) return;
    const node = source.get(id);
    if (!node) return;
    if (visiting.has(id)) {
      depth.set(id, 0);
      lane.set(id, 0);
      return;
    }
    visiting.add(id);
    const parent = node.parentId ? source.get(node.parentId) : undefined;
    if (!parent) {
      depth.set(id, 0);
      lane.set(id, 0);
    } else {
      place(parent.id, visiting);
      depth.set(id, (depth.get(parent.id) ?? 0) + 1);
      if (node.edgeKind === "fork") {
        lane.set(id, nextForkLane++);
      } else if (node.edgeKind === "spawn") {
        lane.set(id, nextAgentLane--);
      } else {
        lane.set(id, lane.get(parent.id) ?? 0);
      }
    }
    visiting.delete(id);
  };

  graph.nodes.forEach((node) => place(node.id));

  const nodes = graph.nodes.map<BranchFlowNode>((node) => ({
    id: node.id,
    type: "branchNode",
    position: {
      x: (lane.get(node.id) ?? 0) * X_GAP,
      y: (depth.get(node.id) ?? 0) * Y_GAP,
    },
    data: { node },
    selectable: true,
    draggable: false,
    focusable: true,
    ariaLabel: `${node.kind}: ${node.title}`,
  }));

  const edges = graph.nodes.flatMap<Edge>((node) => {
    if (!node.parentId || !source.has(node.parentId)) return [];
    return [{
      id: `${node.parentId}:${node.id}`,
      source: node.parentId,
      target: node.id,
      type: "smoothstep",
      className: edgeClass(node.edgeKind),
      animated: node.isCurrent && node.edgeKind !== "spawn",
    }];
  });

  return { nodes, edges };
}
