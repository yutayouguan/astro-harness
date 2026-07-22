import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
  Background,
  Controls,
  MiniMap,
  addEdge,
  useNodesState,
  useEdgesState,
  type Connection,
  type Node as RFNode,
  type Edge as RFEdge,
  type NodeTypes,
  type OnConnect,
  Handle,
  Position,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import {
  ArrowLeft,
  Save,
  Play,
  Plus,
  History,
  ChevronDown,
  ChevronRight,
} from "lucide-react";
import * as LucideIcons from "lucide-react";

type LucideIcon = React.ComponentType<{ size?: number; className?: string }>;
import type { LoopDto, NodeType, NodeMeta } from "./loopTypes";
import { NODE_CATEGORIES, NODE_REGISTRY, getNodesByCategory } from "./loopTypes";
import LoopConfigPanel from "./LoopConfigPanel";
import LoopRunHistory from "./LoopRunHistory";
import LoopRunDetail from "./LoopRunDetail";

interface Props {
  workflowId: string | null;
  providers: { id: string; name: string; model: string; kind: string }[];
  onBack: () => void;
}

// ── Custom node component ────────────────────────────────────────────

function LoopNode({ data, selected }: { data: { label: string; meta: NodeMeta }; selected?: boolean }) {
  const IconComp = (LucideIcons as unknown as Record<string, LucideIcon>)[data.meta.icon];
  return (
    <div
      className={`loop-rf-node loop-rf-node--${data.meta.category}${selected ? " is-selected" : ""}`}
      style={{ "--node-color": data.meta.color } as React.CSSProperties}
    >
      <Handle type="target" position={Position.Left} className="loop-rf-handle" />
      <div className="loop-rf-node-header">
        <span className="loop-rf-node-icon">
          {IconComp && <IconComp size={16} />}
        </span>
        <div className="loop-rf-node-text">
          <span className="loop-rf-node-label">{data.label}</span>
          <span className="loop-rf-node-type-tag">{data.meta.labelEn}</span>
        </div>
      </div>
      <Handle type="source" position={Position.Right} className="loop-rf-handle" />
    </div>
  );
}

const nodeTypes: NodeTypes = {
  loopNode: LoopNode as unknown as NodeTypes[string],
};

// ── Editor (wrapped with ReactFlowProvider) ─────────────────────────

export default function LoopEditor(props: Props) {
  return (
    <ReactFlowProvider>
      <LoopEditorInner {...props} />
    </ReactFlowProvider>
  );
}

function LoopEditorInner({ workflowId, providers: _providers, onBack }: Props) {
  const reactFlowInstance = useReactFlow();
  const [workflow, setWorkflow] = useState<LoopDto | null>(null);
  const [name, setName] = useState("未命名创建loop");
  const [, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);

  const [nodes, setNodes, onNodesChange] = useNodesState<RFNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<RFEdge>([]);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);

  // run history state
  const [showHistory, setShowHistory] = useState(false);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);

  // sidebar collapsed categories
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});

  // custom drag state (HTML5 drag-and-drop doesn't work in Tauri WKWebView)
  const [draggingType, setDraggingType] = useState<NodeType | null>(null);
  const [dragGhostPos, setDragGhostPos] = useState<{ x: number; y: number } | null>(null);

  const reactFlowWrapper = useRef<HTMLDivElement>(null);

  // load workflow
  useEffect(() => {
    if (!workflowId) return;
    (async () => {
      try {
        const wf = await invoke<LoopDto | null>("get_loop", { id: workflowId });
        if (!wf) return;
        setWorkflow(wf);
        setName(wf.name);
        setNodes(
          wf.nodes.map((n) => ({
            id: n.id,
            type: "loopNode",
            position: { x: n.position.x, y: n.position.y },
            data: {
              label: n.label,
              meta: NODE_REGISTRY.find((m) => m.type === n.node_type) ?? NODE_REGISTRY[0],
              config: n.config,
              nodeType: n.node_type,
            },
          })),
        );
        setEdges(
          wf.edges.map((e) => ({
            id: e.id,
            source: e.source,
            sourceHandle: e.source_handle ?? undefined,
            target: e.target,
            targetHandle: e.target_handle ?? undefined,
            animated: true,
          })),
        );
      } catch (e) {
        console.error("get_loop failed", e);
      }
    })();
  }, [workflowId, setNodes, setEdges]);

  const onConnect: OnConnect = useCallback(
    (conn: Connection) => {
      setEdges((eds) => addEdge({ ...conn, animated: true }, eds));
      setDirty(true);
    },
    [setEdges],
  );

  const handleSave = async () => {
    if (!workflow) return;
    setSaving(true);
    try {
      const dto: LoopDto = {
        ...workflow,
        name,
        nodes: nodes.map((n) => ({
          id: n.id,
          node_type: (n.data as Record<string, unknown>).nodeType as NodeType,
          label: (n.data as Record<string, unknown>).label as string,
          position: { x: n.position.x, y: n.position.y },
          config: ((n.data as Record<string, unknown>).config as Record<string, unknown>) ?? {},
          disabled: false,
        })),
        edges: edges.map((e) => ({
          id: e.id,
          source: e.source,
          source_handle: e.sourceHandle ?? null,
          target: e.target,
          target_handle: e.targetHandle ?? null,
        })),
      };
      const saved = await invoke<LoopDto>("save_loop", { data: dto });
      setWorkflow(saved);
      setDirty(false);
    } catch (e) {
      console.error("save_loop failed", e);
    } finally {
      setSaving(false);
    }
  };

  // custom mouse-based drag (bypasses Tauri WKWebView HTML5 DnD issues)
  useEffect(() => {
    if (!draggingType) return;
    const onMouseMove = (e: MouseEvent) => {
      setDragGhostPos({ x: e.clientX, y: e.clientY });
    };
    const onMouseUp = (e: MouseEvent) => {
      const meta = NODE_REGISTRY.find((m) => m.type === draggingType);
      const bounds = reactFlowWrapper.current?.getBoundingClientRect();
      if (meta && bounds && e.clientX >= bounds.left && e.clientX <= bounds.right && e.clientY >= bounds.top && e.clientY <= bounds.bottom) {
        const flowPos = reactFlowInstance.screenToFlowPosition({ x: e.clientX, y: e.clientY });
        const newId = crypto.randomUUID();
        setNodes((nds) => [
          ...nds,
          {
            id: newId,
            type: "loopNode",
            position: flowPos,
            data: { label: meta.label, meta, config: {}, nodeType: draggingType, disabled: false },
          },
        ]);
        setDirty(true);
      }
      setDraggingType(null);
      setDragGhostPos(null);
    };
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
    return () => {
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
  }, [draggingType, reactFlowInstance, setNodes]);

  // 点击 "+" 添加节点到画布中央
  const addNodeToCenter = useCallback(
    (nodeType: NodeType) => {
      const meta = NODE_REGISTRY.find((m) => m.type === nodeType);
      if (!meta) return;
      const center = reactFlowInstance.screenToFlowPosition({
        x: (reactFlowWrapper.current?.clientWidth ?? 600) / 2 + (reactFlowWrapper.current?.getBoundingClientRect().left ?? 0),
        y: (reactFlowWrapper.current?.clientHeight ?? 400) / 2 + (reactFlowWrapper.current?.getBoundingClientRect().top ?? 0),
      });
      const newId = crypto.randomUUID();
      const offset = nodes.length * 20;
      setNodes((nds) => [
        ...nds,
        {
          id: newId,
          type: "loopNode",
          position: { x: center.x + offset, y: center.y + offset },
          data: { label: meta.label, meta, config: {}, nodeType, disabled: false },
        },
      ]);
      setDirty(true);
    },
    [setNodes, reactFlowInstance, nodes.length],
  );

  const toggleCategory = (cat: string) => {
    setCollapsed((prev) => ({ ...prev, [cat]: !prev[cat] }));
  };

  const selectedNode = useMemo(
    () => nodes.find((n) => n.id === selectedNodeId),
    [nodes, selectedNodeId],
  );

  return (
    <div className="loop-editor">
      {/* ── Toolbar ── */}
      <div className="loop-editor-toolbar">
        <button className="loop-icon-btn" onClick={onBack} title="返回">
          <ArrowLeft size={16} />
        </button>
        <input
          className="loop-editor-name"
          value={name}
          onChange={(e) => {
            setName(e.target.value);
            setDirty(true);
          }}
        />
        <div className="loop-editor-toolbar-right">
          <button
            className={`loop-icon-btn${showHistory ? " is-active" : ""}`}
            title="运行日志"
            onClick={() => {
              setShowHistory((v) => !v);
              setSelectedRunId(null);
            }}
          >
            <History size={16} />
          </button>
          <button
            className="loop-btn loop-btn--secondary"
            onClick={() => void handleSave()}
            disabled={saving}
          >
            <Save size={14} />
            <span>{saving ? "保存中…" : "保存"}</span>
          </button>
          <button
            className="loop-btn loop-btn--primary"
            onClick={async () => {
              if (!workflow) return;
              await handleSave();
              try {
                const result = await invoke<{ run_id: string; status: string; steps_executed: number }>("run_loop", { id: workflow.id });
                console.log("run_loop result:", result);
                setShowHistory(true);
              } catch (e) {
                console.error("run_loop failed", e);
              }
            }}
          >
            <Play size={14} />
            <span>运行一次</span>
          </button>
        </div>
      </div>

      <div className="loop-editor-body">
        {/* ── Left: Node palette ── */}
        <div className="loop-node-palette">
          <div className="loop-palette-title">节点</div>
          {NODE_CATEGORIES.map((cat) => {
            const items = getNodesByCategory(cat.key);
            const isCollapsed = !!collapsed[cat.key];
            const CatIcon = (LucideIcons as unknown as Record<string, LucideIcon>)[cat.icon];
            return (
              <div key={cat.key} className={`loop-palette-group${!isCollapsed ? " is-open" : ""}`}>
                <button
                  className="loop-palette-group-header"
                  onClick={() => toggleCategory(cat.key)}
                >
                  {isCollapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
                  {CatIcon && <CatIcon size={14} className="loop-palette-cat-icon" />}
                  <span>{cat.label}</span>
                </button>
                {!isCollapsed && (
                  <div className="loop-palette-items">
                    {items.map((meta) => {
                      const IconComp = (LucideIcons as unknown as Record<string, LucideIcon>)[meta.icon];
                      return (
                        <div
                          key={meta.type}
                          className="loop-palette-item"
                          onMouseDown={(e) => {
                            if (e.button !== 0) return;
                            e.preventDefault();
                            setDraggingType(meta.type);
                            setDragGhostPos({ x: e.clientX, y: e.clientY });
                          }}
                        >
                          <span
                            className="loop-palette-item-icon"
                            style={{ background: `color-mix(in srgb, ${meta.color} 15%, transparent)`, color: meta.color }}
                          >
                            {IconComp && <IconComp size={14} />}
                          </span>
                          <span>{meta.label}</span>
                          <button
                            className="loop-palette-item-plus"
                            onClick={(e) => {
                              e.stopPropagation();
                              addNodeToCenter(meta.type);
                            }}
                          >
                            <Plus size={12} />
                          </button>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            );
          })}
        </div>

        {/* ── Center: Canvas ── */}
        <div className="loop-canvas-container" ref={reactFlowWrapper}>
          <ReactFlow
            nodes={nodes}
            edges={edges}
            onNodesChange={(changes) => {
              onNodesChange(changes);
              setDirty(true);
            }}
            onEdgesChange={(changes) => {
              onEdgesChange(changes);
              setDirty(true);
            }}
            onConnect={onConnect}
            onNodeClick={(_, node) => setSelectedNodeId(node.id)}
            onPaneClick={() => setSelectedNodeId(null)}
            nodeTypes={nodeTypes}
            fitView
            proOptions={{ hideAttribution: true }}
          >
            <Background />
            <Controls />
            <MiniMap
              nodeColor={(n) =>
                ((n.data as Record<string, unknown>)?.meta as NodeMeta)?.color ?? "#888"
              }
            />
          </ReactFlow>
        </div>

        {/* ── Right: Config panel ── */}
        {selectedNode && (
          <LoopConfigPanel
            nodeId={selectedNode.id}
            nodeType={(selectedNode.data as Record<string, unknown>).nodeType as NodeType}
            label={(selectedNode.data as Record<string, unknown>).label as string}
            config={((selectedNode.data as Record<string, unknown>).config as Record<string, unknown>) ?? {}}
            disabled={!!((selectedNode.data as Record<string, unknown>).disabled)}
            onLabelChange={(label) => {
              setNodes((nds) =>
                nds.map((n) =>
                  n.id === selectedNode.id
                    ? { ...n, data: { ...n.data, label } }
                    : n,
                ),
              );
              setDirty(true);
            }}
            onConfigChange={(config) => {
              setNodes((nds) =>
                nds.map((n) =>
                  n.id === selectedNode.id
                    ? { ...n, data: { ...n.data, config } }
                    : n,
                ),
              );
              setDirty(true);
            }}
            onDisabledChange={(disabled) => {
              setNodes((nds) =>
                nds.map((n) =>
                  n.id === selectedNode.id
                    ? { ...n, data: { ...n.data, disabled } }
                    : n,
                ),
              );
              setDirty(true);
            }}
            onDelete={() => {
              setNodes((nds) => nds.filter((n) => n.id !== selectedNode.id));
              setEdges((eds) =>
                eds.filter((e) => e.source !== selectedNode.id && e.target !== selectedNode.id),
              );
              setSelectedNodeId(null);
              setDirty(true);
            }}
            onClose={() => setSelectedNodeId(null)}
          />
        )}

        {/* ── Right: Run history panel ── */}
        {showHistory && !selectedRunId && workflow && (
          <LoopRunHistory
            workflowId={workflow.id}
            onSelectRun={(runId) => setSelectedRunId(runId)}
            onClose={() => setShowHistory(false)}
          />
        )}
        {showHistory && selectedRunId && (
          <LoopRunDetail
            runId={selectedRunId}
            onBack={() => setSelectedRunId(null)}
          />
        )}
      </div>

      {/* ── Drag ghost (follows cursor during custom drag) ── */}
      {draggingType && dragGhostPos && (() => {
        const meta = NODE_REGISTRY.find((m) => m.type === draggingType);
        if (!meta) return null;
        const IconComp = (LucideIcons as unknown as Record<string, LucideIcon>)[meta.icon];
        return (
          <div
            className="loop-drag-ghost"
            style={{ left: dragGhostPos.x, top: dragGhostPos.y }}
          >
            <span
              className="loop-palette-item-icon"
              style={{ background: `color-mix(in srgb, ${meta.color} 15%, transparent)`, color: meta.color }}
            >
              {IconComp && <IconComp size={14} />}
            </span>
            <span>{meta.label}</span>
          </div>
        );
      })()}
    </div>
  );
}
