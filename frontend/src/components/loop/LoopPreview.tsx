import { useCallback, useEffect, useMemo, useRef } from "react";
import {
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
  Background,
  BackgroundVariant,
  Handle,
  Position,
  type Node as RFNode,
  type Edge as RFEdge,
  type NodeTypes,
  type ReactFlowInstance,
} from "@xyflow/react";
import * as LucideIcons from "lucide-react";
import type { LucideIcon } from "lucide-react";
import type { LoopDto, LoopNodeDto, LoopEdgeDto } from "./loopTypes";
import { NODE_REGISTRY, type NodeMeta } from "./loopTypes";

// ── DAG layered layout ──────────────────────────────────────────

const LAYER_GAP_X = 200;
const NODE_GAP_Y = 70;

function layoutNodes(
  nodes: LoopNodeDto[],
  edges: LoopEdgeDto[],
): Map<string, { x: number; y: number }> {
  const ids = new Set(nodes.map((n) => n.id));
  const children: Map<string, string[]> = new Map();
  const inDeg: Map<string, number> = new Map();

  for (const id of ids) {
    children.set(id, []);
    inDeg.set(id, 0);
  }
  for (const e of edges) {
    if (!ids.has(e.source) || !ids.has(e.target)) continue;
    children.get(e.source)!.push(e.target);
    inDeg.set(e.target, (inDeg.get(e.target) ?? 0) + 1);
  }

  const layers: string[][] = [];
  const depth: Map<string, number> = new Map();

  // BFS topological sort — longest-path layering
  const queue: string[] = [];
  for (const id of ids) {
    if (inDeg.get(id) === 0) {
      queue.push(id);
      depth.set(id, 0);
    }
  }

  while (queue.length > 0) {
    const cur = queue.shift()!;
    const d = depth.get(cur)!;
    for (const child of children.get(cur)!) {
      const prev = depth.get(child);
      if (prev === undefined || d + 1 > prev) {
        depth.set(child, d + 1);
      }
      inDeg.set(child, inDeg.get(child)! - 1);
      if (inDeg.get(child) === 0) {
        queue.push(child);
      }
    }
  }

  // Nodes not reached (cycles / isolated) get appended
  for (const id of ids) {
    if (!depth.has(id)) depth.set(id, (layers.length || 1) - 1);
  }

  for (const [id, d] of depth) {
    while (layers.length <= d) layers.push([]);
    layers[d].push(id);
  }

  // Assign positions — center each layer vertically
  const positions = new Map<string, { x: number; y: number }>();
  for (let col = 0; col < layers.length; col++) {
    const layer = layers[col];
    const totalHeight = (layer.length - 1) * NODE_GAP_Y;
    const startY = -totalHeight / 2;
    for (let row = 0; row < layer.length; row++) {
      positions.set(layer[row], {
        x: col * LAYER_GAP_X,
        y: startY + row * NODE_GAP_Y,
      });
    }
  }

  return positions;
}

// ── Preview node ────────────────────────────────────────────────

interface PreviewNodeData {
  label: string;
  meta: NodeMeta;
  sourceHandleIds: string[];
}

function PreviewNode({ data }: { data: PreviewNodeData }) {
  const IconComp = (LucideIcons as unknown as Record<string, LucideIcon>)[data.meta.icon];
  return (
    <div
      className="loop-preview-node"
      style={{ "--node-color": data.meta.color } as React.CSSProperties}
    >
      <Handle type="target" position={Position.Left} className="loop-preview-handle" />
      <span className="loop-preview-node-icon">
        {IconComp && <IconComp size={11} />}
      </span>
      <div className="loop-preview-node-text">
        <span className="loop-preview-node-label">{data.label}</span>
        <span className="loop-preview-node-type">{data.meta.labelEn}</span>
      </div>
      {data.sourceHandleIds.length > 0
        ? data.sourceHandleIds.map((hid) => (
            <Handle key={hid} type="source" position={Position.Right} id={hid} className="loop-preview-handle" />
          ))
        : <Handle type="source" position={Position.Right} className="loop-preview-handle" />
      }
    </div>
  );
}

const previewNodeTypes: NodeTypes = {
  preview: PreviewNode as unknown as NodeTypes[string],
};

// ── Preview component ───────────────────────────────────────────

interface Props {
  workflow: LoopDto;
}

function LoopPreviewInner({ workflow }: Props) {
  const { fitView } = useReactFlow();
  const rfRef = useRef<ReactFlowInstance | null>(null);

  useEffect(() => {
    requestAnimationFrame(() => fitView({ padding: 0.12, maxZoom: 1, duration: 300 }));
  }, [workflow.id, fitView]);

  const onInit = useCallback((instance: ReactFlowInstance) => {
    rfRef.current = instance;
    instance.fitView({ padding: 0.12, maxZoom: 1 });
  }, []);

  const sourceHandleMap = useMemo(() => {
    const map: Record<string, string[]> = {};
    for (const e of workflow.edges) {
      if (e.source_handle) {
        (map[e.source] ??= []).push(e.source_handle);
      }
    }
    for (const k of Object.keys(map)) {
      map[k] = [...new Set(map[k])];
    }
    return map;
  }, [workflow.edges]);

  const positions = useMemo(
    () => layoutNodes(workflow.nodes, workflow.edges),
    [workflow.nodes, workflow.edges],
  );

  const nodes: RFNode[] = useMemo(
    () =>
      workflow.nodes.map((n) => {
        const pos = positions.get(n.id) ?? { x: n.position.x, y: n.position.y };
        return {
          id: n.id,
          type: "preview",
          position: pos,
          data: {
            label: n.label,
            meta: NODE_REGISTRY.find((m) => m.type === n.node_type) ?? NODE_REGISTRY[0],
            sourceHandleIds: sourceHandleMap[n.id] ?? [],
          },
        };
      }),
    [workflow.nodes, positions, sourceHandleMap],
  );

  const edges: RFEdge[] = useMemo(
    () =>
      workflow.edges.map((e) => ({
        id: e.id,
        source: e.source,
        sourceHandle: e.source_handle ?? undefined,
        target: e.target,
        targetHandle: e.target_handle ?? undefined,
        animated: true,
      })),
    [workflow.edges],
  );

  if (nodes.length === 0) return null;

  return (
    <ReactFlow
      nodes={nodes}
      edges={edges}
      nodeTypes={previewNodeTypes}
      onInit={onInit}
      fitView
      fitViewOptions={{ padding: 0.12, maxZoom: 1 }}
      minZoom={0.1}
      nodesDraggable={false}
      nodesConnectable={false}
      nodesFocusable={false}
      edgesFocusable={false}
      elementsSelectable={false}
      panOnDrag
      zoomOnScroll
      zoomOnPinch
      zoomOnDoubleClick={false}
      preventScrolling={false}
      proOptions={{ hideAttribution: true }}
    >
      <Background variant={BackgroundVariant.Dots} gap={16} size={0.8} color="var(--ink-faint, #d4d4d8)" />
    </ReactFlow>
  );
}

export default function LoopPreview({ workflow }: Props) {
  return (
    <div className="loop-preview-container">
      <ReactFlowProvider>
        <LoopPreviewInner workflow={workflow} />
      </ReactFlowProvider>
    </div>
  );
}
