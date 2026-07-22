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
import { NODE_CATEGORIES, NODE_REGISTRY, getNodesByCategory, getNodeMeta } from "./loopTypes";
import LoopConfigPanel from "./LoopConfigPanel";
import LoopRunHistory from "./LoopRunHistory";
import LoopRunDetail from "./LoopRunDetail";

interface Props {
  workflowId: string | null;
  providers: { id: string; name: string; model: string; kind: string }[];
  onBack: () => void;
}

// ── Custom node component ────────────────────────────────────────────

interface LoopNodeData {
  label: string;
  meta: NodeMeta;
  disabled?: boolean;
  onRunNode?: () => void;
  onToggleDisable?: () => void;
  onDeleteNode?: () => void;
}

function LoopNode({ data, selected }: { data: LoopNodeData; selected?: boolean }) {
  const IconComp = (LucideIcons as unknown as Record<string, LucideIcon>)[data.meta.icon];
  return (
    <div
      className={`loop-rf-node${selected ? " is-selected" : ""}${data.disabled ? " is-disabled" : ""}`}
      style={{ "--node-color": data.meta.color } as React.CSSProperties}
    >
      {/* 选中时顶部工具栏 */}
      {selected && (
        <div className="loop-rf-node-toolbar">
          <button
            className="loop-rf-toolbar-btn"
            title="运行此节点"
            onClick={(e) => { e.stopPropagation(); data.onRunNode?.(); }}
          >
            <LucideIcons.Play size={12} />
          </button>
          <button
            className={`loop-rf-toolbar-btn${data.disabled ? " is-active" : ""}`}
            title={data.disabled ? "启用" : "禁用"}
            onClick={(e) => { e.stopPropagation(); data.onToggleDisable?.(); }}
          >
            <LucideIcons.Power size={12} />
          </button>
          <button
            className="loop-rf-toolbar-btn loop-rf-toolbar-btn--danger"
            title="删除"
            onClick={(e) => { e.stopPropagation(); data.onDeleteNode?.(); }}
          >
            <LucideIcons.Trash2 size={12} />
          </button>
        </div>
      )}
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

  // 已保存的工作流列表（自定义分组使用）
  const [savedLoops, setSavedLoops] = useState<LoopDto[]>([]);

  // run history state
  const [showHistory, setShowHistory] = useState(false);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);

  // node action callbacks (injected into node data for toolbar buttons)
  const deleteNode = useCallback((nodeId: string) => {
    setNodes((nds) => nds.filter((n) => n.id !== nodeId));
    setEdges((eds) => eds.filter((e) => e.source !== nodeId && e.target !== nodeId));
    setSelectedNodeId(null);
    setDirty(true);
  }, [setNodes, setEdges]);

  const toggleNodeDisabled = useCallback((nodeId: string) => {
    setNodes((nds) =>
      nds.map((n) =>
        n.id === nodeId
          ? { ...n, data: { ...n.data, disabled: !(n.data as Record<string, unknown>).disabled } }
          : n,
      ),
    );
    setDirty(true);
  }, [setNodes]);

  // inject callbacks into nodes so the toolbar buttons work
  const nodesWithCallbacks = useMemo(
    () =>
      nodes.map((n) => ({
        ...n,
        data: {
          ...n.data,
          onRunNode: () => console.log("run node", n.id),
          onToggleDisable: () => toggleNodeDisabled(n.id),
          onDeleteNode: () => deleteNode(n.id),
        },
      })),
    [nodes, deleteNode, toggleNodeDisabled],
  );

  // sidebar collapsed categories
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});

  // custom drag state (HTML5 drag-and-drop doesn't work in Tauri WKWebView)
  const [draggingType, setDraggingType] = useState<NodeType | null>(null);
  const [dragGhostPos, setDragGhostPos] = useState<{ x: number; y: number } | null>(null);

  const reactFlowWrapper = useRef<HTMLDivElement>(null);
  const customLoopRef = useRef<string | null>(null);

  // load saved loops for custom category
  useEffect(() => {
    (async () => {
      try {
        const list = await invoke<LoopDto[]>("list_loops");
        setSavedLoops(list.filter((l) => l.id !== workflowId));
      } catch { /* ignore */ }
    })();
  }, [workflowId]);

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
        // custom_loop 节点使用引用的工作流名称和 ID
        const refLoopId = customLoopRef.current;
        const refLoop = refLoopId ? savedLoops.find((l) => l.id === refLoopId) : null;
        const nodeLabel = refLoop ? refLoop.name : meta.label;
        const nodeConfig = refLoop ? { workflow_id: refLoopId } : {};
        setNodes((nds) => [
          ...nds,
          {
            id: newId,
            type: "loopNode",
            position: flowPos,
            data: {
              label: nodeLabel,
              meta: refLoop ? { ...meta, label: nodeLabel } : meta,
              config: nodeConfig,
              nodeType: draggingType,
              disabled: false,
            },
          },
        ]);
        setDirty(true);
      }
      setDraggingType(null);
      setDragGhostPos(null);
      customLoopRef.current = null;
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

  const addCustomLoopNode = useCallback(
    (loopId: string, loopName: string) => {
      const meta = getNodeMeta("custom_loop");
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
          data: {
            label: loopName,
            meta: { ...meta, label: loopName },
            config: { workflow_id: loopId },
            nodeType: "custom_loop" as NodeType,
            disabled: false,
          },
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
                    {/* 静态节点列表 */}
                    {items.filter((m) => m.type !== "custom_loop").map((meta) => {
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
                    {/* 自定义分组：已保存的工作流 */}
                    {cat.key === "custom" && savedLoops.map((lp) => (
                      <div
                        key={lp.id}
                        className="loop-palette-item"
                        onMouseDown={(e) => {
                          if (e.button !== 0) return;
                          e.preventDefault();
                          setDraggingType("custom_loop");
                          setDragGhostPos({ x: e.clientX, y: e.clientY });
                          customLoopRef.current = lp.id;
                        }}
                      >
                        <span
                          className="loop-palette-item-icon"
                          style={{ background: "color-mix(in srgb, #8b5cf6 15%, transparent)", color: "#8b5cf6" }}
                        >
                          <LucideIcons.Workflow size={14} />
                        </span>
                        <span>{lp.name}</span>
                        <button
                          className="loop-palette-item-plus"
                          onClick={(e) => {
                            e.stopPropagation();
                            addCustomLoopNode(lp.id, lp.name);
                          }}
                        >
                          <Plus size={12} />
                        </button>
                      </div>
                    ))}
                    {cat.key === "custom" && savedLoops.length === 0 && (
                      <div className="loop-palette-empty">
                        把任意节点保存为预设，或已有的 Loop 会出现在这里
                      </div>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>

        {/* ── Center: Canvas ── */}
        <div className="loop-canvas-container" ref={reactFlowWrapper}>
          <ReactFlow
            nodes={nodesWithCallbacks}
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
