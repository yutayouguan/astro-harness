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
import { LOOP_ICON_MAP } from "./loopIcons";
import type { LoopDto } from "./loopTypes";
import { NODE_REGISTRY, type NodeMeta } from "./loopTypes";
import { layoutNodes } from "./loopLayout";

interface PreviewNodeData {
  label: string;
  meta: NodeMeta;
  sourceHandleIds: string[];
}

function PreviewNode({ data }: { data: PreviewNodeData }) {
  const IconComp = LOOP_ICON_MAP[data.meta.icon];
  return (
    <div
      className="loop-preview-node"
      style={{ "--node-color": data.meta.color } as React.CSSProperties}
    >
      <Handle
        type="target"
        position={Position.Left}
        className="loop-preview-handle"
      />
      <span className="loop-preview-node-icon">
        {IconComp && <IconComp size={11} />}
      </span>
      <div className="loop-preview-node-text">
        <span className="loop-preview-node-label">{data.label}</span>
        <span className="loop-preview-node-type">{data.meta.labelEn}</span>
      </div>
      {data.sourceHandleIds.length > 0 ? (
        data.sourceHandleIds.map((hid) => (
          <Handle
            key={hid}
            type="source"
            position={Position.Right}
            id={hid}
            className="loop-preview-handle"
          />
        ))
      ) : (
        <Handle
          type="source"
          position={Position.Right}
          className="loop-preview-handle"
        />
      )}
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
    requestAnimationFrame(() =>
      fitView({ padding: 0.12, maxZoom: 1, duration: 300 }),
    );
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
            meta:
              NODE_REGISTRY.find((m) => m.type === n.node_type) ??
              NODE_REGISTRY[0],
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
      <Background
        variant={BackgroundVariant.Dots}
        gap={16}
        size={0.8}
        color="var(--ink-faint, #d4d4d8)"
      />
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
