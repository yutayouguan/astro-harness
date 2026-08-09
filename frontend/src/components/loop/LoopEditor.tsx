import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
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
  ConnectionLineType,
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
import type { LoopDto, NodeType, NodeMeta, LoopIconData } from "./loopTypes";
import { NODE_CATEGORIES, NODE_REGISTRY, getNodesByCategory, getNodeMeta, parseLoopIcon, serializeLoopIcon } from "./loopTypes";
import { clampPopover, pointAnchor, measurePopoverSize } from "../../lib/ui/clampPopover";
import LoopConfigPanel from "./LoopConfigPanel";
import { computeUpstreamOutputs } from "./configs/upstreamOutputs";
import { validateNodeConfig } from "./loopValidation";
import LoopRunHistory from "./LoopRunHistory";
import LoopRunDetail from "./LoopRunDetail";
import LoopIcon from "./LoopIcon";
import LoopAiAssistant from "./LoopAiAssistant";
import { layoutNodes } from "./loopLayout";
import { useLoopHistory } from "./useLoopHistory";
import { LucideIconPicker } from "../agents";
import { toneStyleFromElement } from "../../lib/ui/toneFromElement";
import { useTransientToast } from "../../hooks/ui/useTransientToast";
import { useI18n } from "../../i18n/LocaleContext";
import { useConfirm } from "../../hooks/ui/DialogContext";

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
  const { t } = useI18n();
  const IconComp = (LucideIcons as unknown as Record<string, LucideIcon>)[data.meta.icon];
  const raw = data as unknown as Record<string, unknown>;
  const warnings = validateNodeConfig(
    (raw.nodeType as NodeType) ?? data.meta.type,
    (raw.config as Record<string, unknown>) ?? {},
  );
  return (
    <div
      className={`loop-rf-node${selected ? " is-selected" : ""}${data.disabled ? " is-disabled" : ""}`}
      style={{ "--node-color": data.meta.color } as React.CSSProperties}
    >
      {selected && (
        <div className="loop-rf-node-toolbar">
          <button
            className={`loop-rf-toolbar-btn${data.disabled ? " is-active" : ""}`}
            title={data.disabled ? t("loop.enabledYes") : t("loop.enabledNo")}
            onClick={(e) => { e.stopPropagation(); data.onToggleDisable?.(); }}
          >
            <LucideIcons.Power size={12} />
          </button>
          <button
            className="loop-rf-toolbar-btn loop-rf-toolbar-btn--danger"
            title={t("loop.delete")}
            onClick={(e) => { e.stopPropagation(); data.onDeleteNode?.(); }}
          >
            <LucideIcons.Trash2 size={12} />
          </button>
        </div>
      )}
      {warnings.length > 0 && (
        <span className="loop-rf-node-warn" title={warnings.join("、") + " 未配置"}>
          <LucideIcons.AlertTriangle size={10} />
        </span>
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

interface BranchCondition {
  id: string;
  label?: string;
  expression?: string;
}

function BranchNode({ data, selected }: { data: LoopNodeData; selected?: boolean }) {
  const { t } = useI18n();
  const IconComp = (LucideIcons as unknown as Record<string, LucideIcon>)[data.meta.icon];
  const config = (data as unknown as Record<string, unknown>).config as Record<string, unknown> | undefined;
  const conditionsRaw = config?.conditions ?? config?.branches;
  let branches: BranchCondition[] = [];
  if (typeof conditionsRaw === "string") {
    try { branches = JSON.parse(conditionsRaw); } catch { /* ignore */ }
  } else if (Array.isArray(conditionsRaw)) {
    branches = conditionsRaw as BranchCondition[];
  }
  if (branches.length === 0) branches = [{ id: "default", label: t("loop.branchDefault") }];

  return (
    <div
      className={`loop-rf-node loop-rf-node--branch${selected ? " is-selected" : ""}${data.disabled ? " is-disabled" : ""}`}
      style={{ "--node-color": data.meta.color } as React.CSSProperties}
    >
      {selected && (
        <div className="loop-rf-node-toolbar">
          <button className={`loop-rf-toolbar-btn${data.disabled ? " is-active" : ""}`}
            title={data.disabled ? t("loop.enabledYes") : t("loop.enabledNo")}
            onClick={(e) => { e.stopPropagation(); data.onToggleDisable?.(); }}>
            <LucideIcons.Power size={12} />
          </button>
          <button className="loop-rf-toolbar-btn loop-rf-toolbar-btn--danger" title={t("loop.delete")}
            onClick={(e) => { e.stopPropagation(); data.onDeleteNode?.(); }}>
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
      <div className="loop-rf-branch-labels">
        {branches.map((b) => (
          <span key={b.id} className="loop-rf-branch-label">{b.label || b.id}</span>
        ))}
      </div>
      {branches.map((b) => (
        <Handle
          key={b.id}
          type="source"
          position={Position.Right}
          id={b.id}
          className="loop-rf-handle"
        />
      ))}
    </div>
  );
}

const BRANCH_NODE_TYPES: Set<string> = new Set(["conditional", "multi_branch", "question_classification"]);

function rfNodeType(nodeType: string): string {
  return BRANCH_NODE_TYPES.has(nodeType) ? "branchNode" : "loopNode";
}

const nodeTypes: NodeTypes = {
  loopNode: LoopNode as unknown as NodeTypes[string],
  branchNode: BranchNode as unknown as NodeTypes[string],
};

// ── Connection-drop node picker popup ───────────────────────────────

interface NodePickerProps {
  x: number;
  y: number;
  savedLoops: LoopDto[];
  onSelect: (nodeType: NodeType, label?: string, config?: Record<string, unknown>) => void;
  onClose: () => void;
}

function LoopNodePicker({ x, y, savedLoops, onSelect, onClose }: NodePickerProps) {
  const { t } = useI18n();
  const menuRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const [search, setSearch] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useLayoutEffect(() => {
    if (!menuRef.current) return;
    const size = measurePopoverSize(menuRef.current);
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const bounds = { left: 0, top: 0, right: vw, bottom: vh };
    const result = clampPopover({
      anchorRect: pointAnchor(x, y),
      popoverSize: size,
      bounds,
      pad: 12,
      gap: 0,
    });
    setPos({ left: result.left, top: result.top });
  }, [x, y]);

  useEffect(() => {
    const raf = requestAnimationFrame(() => inputRef.current?.focus());
    return () => cancelAnimationFrame(raf);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    const onPointer = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onPointer);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onPointer);
    };
  }, [onClose]);

  const q = search.toLowerCase().trim();

  const menu = (
    <div
      ref={menuRef}
      className="loop-node-picker"
      style={{
        position: "fixed",
        left: pos?.left ?? x,
        top: pos?.top ?? y,
        zIndex: 9999,
        visibility: pos ? "visible" : "hidden",
      }}
    >
      <div className="loop-node-picker-search">
        <LucideIcons.Search size={14} className="loop-node-picker-search-icon" />
        <input
          ref={inputRef}
          type="text"
          className="loop-node-picker-search-input"
          placeholder={t("loop.searchNodes")}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>
      <div className="loop-node-picker-body">
        {NODE_CATEGORIES.filter((cat) => cat.key !== "custom").map((cat) => {
          const items = getNodesByCategory(cat.key).filter(
            (m) => !q || m.label.toLowerCase().includes(q) || m.labelEn.toLowerCase().includes(q),
          );
          if (items.length === 0) return null;
          const CatIcon = (LucideIcons as unknown as Record<string, LucideIcon>)[cat.icon];
          return (
            <div key={cat.key} className="loop-node-picker-group">
              <div className="loop-node-picker-group-header">
                {CatIcon && <CatIcon size={12} />}
                <span>{cat.label}</span>
              </div>
              {items.map((meta) => {
                const IconComp = (LucideIcons as unknown as Record<string, LucideIcon>)[meta.icon];
                return (
                  <button
                    key={meta.type}
                    className="loop-node-picker-item"
                    onClick={() => onSelect(meta.type)}
                  >
                    <span className="loop-node-picker-item-icon" style={{ color: meta.color }}>
                      {IconComp && <IconComp size={16} />}
                    </span>
                    <span>{meta.label}</span>
                  </button>
                );
              })}
            </div>
          );
        })}
        {savedLoops.length > 0 && (
          <div className="loop-node-picker-group">
            <div className="loop-node-picker-group-header">
              <LucideIcons.Workflow size={12} />
              <span>{t("loop.savedFlows")}</span>
            </div>
            {savedLoops
              .filter((lp) => !q || lp.name.toLowerCase().includes(q))
              .map((lp) => (
                <button
                  key={lp.id}
                  className="loop-node-picker-item"
                  onClick={() => onSelect("custom_loop" as NodeType, lp.name, { workflow_id: lp.id })}
                >
                  <span className="loop-node-picker-item-icon" style={{ color: "#8b5cf6" }}>
                    <LucideIcons.Workflow size={16} />
                  </span>
                  <span>{lp.name}</span>
                </button>
              ))}
          </div>
        )}
      </div>
    </div>
  );

  return createPortal(menu, document.body);
}

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
  const { t } = useI18n();
  const { showToast, toastHost } = useTransientToast();
  const confirm = useConfirm();
  const [workflow, setWorkflow] = useState<LoopDto | null>(null);
  const [name, setName] = useState(t("loop.defaultName"));
  const [description, setDescription] = useState("");
  const [variables, setVariables] = useState<Record<string, unknown>>({});
  const [showVarsPanel, setShowVarsPanel] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [editingName, setEditingName] = useState(false);
  const [iconData, setIconData] = useState<LoopIconData | null>(null);
  const [iconPickerOpen, setIconPickerOpen] = useState(false);
  const editorRef = useRef<HTMLDivElement>(null);
  const nameInputRef = useRef<HTMLInputElement>(null);
  const [fullscreen, setFullscreen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(true);
  const [paletteSearch, setPaletteSearch] = useState("");
  const [showShortcuts, setShowShortcuts] = useState(false);

  const [nodes, setNodes, onNodesChange] = useNodesState<RFNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<RFEdge>([]);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  const { pushSnapshot, undo, redo } = useLoopHistory(
    setNodes, setEdges,
    () => reactFlowInstance.getNodes(),
    () => reactFlowInstance.getEdges(),
  );

  const clipboardRef = useRef<{ nodes: RFNode[]; edges: RFEdge[] } | null>(null);
  const saveRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;
      const mod = e.metaKey || e.ctrlKey;
      if (!mod) {
        if (e.key === "Delete" || e.key === "Backspace") {
          const selected = reactFlowInstance.getNodes().filter((n) => n.selected);
          if (selected.length > 0) {
            pushSnapshot();
            const ids = new Set(selected.map((n) => n.id));
            setNodes((nds) => nds.filter((n) => !ids.has(n.id)));
            setEdges((eds) => eds.filter((ed) => !ids.has(ed.source) && !ids.has(ed.target)));
            setSelectedNodeId(null);
            setDirty(true);
          }
        }
        return;
      }

      if (e.key === "z" && !e.shiftKey) {
        e.preventDefault();
        undo();
      } else if ((e.key === "z" && e.shiftKey) || e.key === "y") {
        e.preventDefault();
        redo();
      } else if (e.key === "a") {
        e.preventDefault();
        setNodes((nds) => nds.map((n) => ({ ...n, selected: true })));
      } else if (e.key === "c") {
        const selected = reactFlowInstance.getNodes().filter((n) => n.selected);
        if (selected.length === 0) return;
        const selectedIds = new Set(selected.map((n) => n.id));
        const relatedEdges = reactFlowInstance.getEdges().filter(
          (e) => selectedIds.has(e.source) && selectedIds.has(e.target),
        );
        clipboardRef.current = { nodes: selected, edges: relatedEdges };
      } else if (e.key === "v" && clipboardRef.current) {
        e.preventDefault();
        const { nodes: clipNodes, edges: clipEdges } = clipboardRef.current;
        const idMap = new Map<string, string>();
        clipNodes.forEach((n) => idMap.set(n.id, `${n.id}_copy_${Date.now()}`));
        const newNodes = clipNodes.map((n) => ({
          ...n,
          id: idMap.get(n.id)!,
          position: { x: n.position.x + 40, y: n.position.y + 40 },
          selected: false,
        }));
        const newEdges = clipEdges
          .filter((e) => idMap.has(e.source) && idMap.has(e.target))
          .map((e) => ({
            ...e,
            id: `${e.id}_copy_${Date.now()}`,
            source: idMap.get(e.source)!,
            target: idMap.get(e.target)!,
          }));
        pushSnapshot();
        setNodes((nds) => [...nds, ...newNodes]);
        setEdges((eds) => [...eds, ...newEdges]);
        setDirty(true);
      } else if (e.key === "s") {
        e.preventDefault();
        saveRef.current?.();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [undo, redo, reactFlowInstance, setNodes, setEdges, pushSnapshot, setSelectedNodeId]);

  const selectedUpstreamOutputs = useMemo(() => {
    if (!selectedNodeId) return [];
    const allNodes = nodes.map((n) => {
      const d = n.data as Record<string, unknown>;
      return { id: n.id, nodeType: d.nodeType as NodeType, label: (d.label as string) || n.id };
    });
    const simpleEdges = edges.map((e) => ({ source: e.source, target: e.target }));
    return computeUpstreamOutputs(selectedNodeId, allNodes, simpleEdges);
  }, [selectedNodeId, nodes, edges]);

  // 已保存的工作流列表（自定义分组使用）
  const [savedLoops, setSavedLoops] = useState<LoopDto[]>([]);

  // run history state
  const [showHistory, setShowHistory] = useState(false);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [showAiAssistant, setShowAiAssistant] = useState(false);

  const handleAutoLayout = useCallback(() => {
    pushSnapshot();
    const curNodes = reactFlowInstance.getNodes();
    const curEdges = reactFlowInstance.getEdges();
    const dtoNodes = curNodes.map((n) => ({
      id: n.id,
      node_type: (n.data as Record<string, unknown>).nodeType as NodeType,
      label: (n.data as Record<string, unknown>).label as string,
      position: n.position,
      config: {} as Record<string, unknown>,
      disabled: false,
    }));
    const dtoEdges = curEdges.map((e) => ({
      id: e.id,
      source: e.source,
      source_handle: e.sourceHandle ?? null,
      target: e.target,
      target_handle: e.targetHandle ?? null,
    }));
    const positions = layoutNodes(dtoNodes, dtoEdges);
    setNodes((nds) =>
      nds.map((n) => {
        const pos = positions.get(n.id);
        return pos ? { ...n, position: pos } : n;
      }),
    );
    setDirty(true);
    setTimeout(() => reactFlowInstance.fitView({ padding: 0.15, duration: 300 }), 50);
  }, [reactFlowInstance, setNodes]);

  // node action callbacks (injected into node data for toolbar buttons)
  const deleteNode = useCallback((nodeId: string) => {
    pushSnapshot();
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

  // inject callbacks + validation state into nodes
  const nodesWithCallbacks = useMemo(
    () =>
      nodes.map((n) => {
        const d = n.data as Record<string, unknown>;
        const nodeType = d.nodeType as NodeType;
        const cfg = (d.config as Record<string, unknown>) ?? {};
        return {
          ...n,
          data: {
            ...n.data,
            hasErrors: validateNodeConfig(nodeType, cfg).length > 0,
            onToggleDisable: () => toggleNodeDisabled(n.id),
            onDeleteNode: async () => {
              const ok = await confirm({ title: t("loop.deleteNode"), message: t("loop.deleteNodeConfirm").replace("{name}", String(d.label)), confirmLabel: t("loop.delete"), variant: "danger" });
              if (ok) deleteNode(n.id);
            },
          },
        };
      }),
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
        setDescription(wf.description ?? "");
        setVariables(wf.variables ?? {});
        setIconData(parseLoopIcon(wf.icon));
        setNodes(
          wf.nodes.map((n) => ({
            id: n.id,
            type: rfNodeType(n.node_type),
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
            ...(e.source_handle ? { label: e.source_handle, labelStyle: { fontSize: 10, fill: "var(--ink-tertiary)" } } : {}),
          })),
        );
      } catch (e) {
        showToast(String(e), { tone: "error" });
      }
    })();
  }, [workflowId, setNodes, setEdges]);

  const onConnect: OnConnect = useCallback(
    (conn: Connection) => {
      pushSnapshot();
      setEdges((eds) => addEdge({ ...conn, animated: true }, eds));
      setDirty(true);
    },
    [setEdges, pushSnapshot],
  );

  // ── Connection-drop node picker ──────────────────────────────────
  const [connectDrop, setConnectDrop] = useState<{
    x: number; y: number;
    flowPos: { x: number; y: number };
    sourceNodeId: string;
    sourceHandleId: string | null;
  } | null>(null);

  const connectStartRef = useRef<{ nodeId: string; handleId: string | null } | null>(null);

  const onConnectStart = useCallback(
    (_: unknown, params: { nodeId: string | null; handleId: string | null }) => {
      if (params.nodeId) {
        connectStartRef.current = { nodeId: params.nodeId, handleId: params.handleId };
      }
    },
    [],
  );

  const onConnectEnd = useCallback(
    (event: MouseEvent | TouchEvent) => {
      const src = connectStartRef.current;
      connectStartRef.current = null;
      if (!src) return;

      const target = (event as MouseEvent).target as Element | null;
      if (target?.closest(".react-flow__handle")) return;

      const clientX = "clientX" in event ? event.clientX : event.changedTouches?.[0]?.clientX ?? 0;
      const clientY = "clientY" in event ? event.clientY : event.changedTouches?.[0]?.clientY ?? 0;

      const bounds = reactFlowWrapper.current?.getBoundingClientRect();
      if (!bounds) return;
      if (clientX < bounds.left || clientX > bounds.right || clientY < bounds.top || clientY > bounds.bottom) return;

      const flowPos = reactFlowInstance.screenToFlowPosition({ x: clientX, y: clientY });
      setConnectDrop({
        x: clientX,
        y: clientY,
        flowPos,
        sourceNodeId: src.nodeId,
        sourceHandleId: src.handleId,
      });
    },
    [reactFlowInstance],
  );

  const handleNodePickerSelect = useCallback(
    (nodeType: NodeType, overrideLabel?: string, overrideConfig?: Record<string, unknown>) => {
      if (!connectDrop) return;
      const meta = getNodeMeta(nodeType);
      const newId = crypto.randomUUID();
      setNodes((nds) => [
        ...nds,
        {
          id: newId,
          type: rfNodeType(nodeType),
          position: connectDrop.flowPos,
          data: {
            label: overrideLabel ?? meta.label,
            meta: overrideLabel ? { ...meta, label: overrideLabel } : meta,
            config: overrideConfig ?? {},
            nodeType,
            disabled: false,
          },
        },
      ]);
      setEdges((eds) =>
        addEdge(
          { id: crypto.randomUUID(), source: connectDrop.sourceNodeId, sourceHandle: connectDrop.sourceHandleId ?? undefined, target: newId, animated: true },
          eds,
        ),
      );
      setDirty(true);
      setConnectDrop(null);
    },
    [connectDrop, setNodes, setEdges],
  );

  const handleSave = async () => {
    if (!workflow) return;
    setSaving(true);
    try {
      const dto: LoopDto = {
        ...workflow,
        name,
        description,
        variables,
        icon: iconData ? serializeLoopIcon(iconData) : undefined,
        nodes: nodes.map((n) => ({
          id: n.id,
          node_type: (n.data as Record<string, unknown>).nodeType as NodeType,
          label: (n.data as Record<string, unknown>).label as string,
          position: { x: n.position.x, y: n.position.y },
          config: ((n.data as Record<string, unknown>).config as Record<string, unknown>) ?? {},
          disabled: !!(n.data as Record<string, unknown>).disabled,
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
      showToast(t("loop.saved"), { tone: "success" });
    } catch (e) {
      showToast(String(e), { tone: "error" });
    } finally {
      setSaving(false);
    }
  };

  saveRef.current = () => void handleSave();

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
            type: rfNodeType(draggingType),
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
          type: rfNodeType(nodeType),
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
          type: rfNodeType("custom_loop"),
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
    <div ref={editorRef} className={`loop-editor${fullscreen ? " loop-editor--fullscreen" : ""}`}>
      {/* ── Toolbar ── */}
      <div className="loop-editor-toolbar">
        <button className="loop-icon-btn" onClick={async () => {
          if (dirty) {
            const ok = await confirm({ title: t("loop.unsavedTitle"), message: t("loop.unsavedMessage"), confirmLabel: t("loop.unsavedLeave"), variant: "danger" });
            if (!ok) return;
          }
          onBack();
        }} title={t("loop.back")}>
          <ArrowLeft size={16} />
        </button>
        <button
          className="loop-editor-icon-btn"
          title={t("loop.selectIcon")}
          onClick={() => setIconPickerOpen(true)}
        >
          <LoopIcon icon={iconData} size={20} />
        </button>
        <div className="loop-editor-name-wrap">
          {editingName ? (
            <input
              ref={nameInputRef}
              className="loop-editor-name loop-editor-name--editing"
              value={name}
              autoFocus
              onChange={(e) => {
                setName(e.target.value);
                setDirty(true);
              }}
              onBlur={() => setEditingName(false)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === "Escape") {
                  setEditingName(false);
                  (e.target as HTMLInputElement).blur();
                }
              }}
            />
          ) : (
            <button
              className="loop-editor-name-display"
              onClick={() => setEditingName(true)}
            >
              <span>{name}</span>
              <LucideIcons.Pencil size={12} className="loop-editor-name-edit" />
            </button>
          )}
        </div>
        <input
          className="loop-editor-desc"
          value={description}
          onChange={(e) => { setDescription(e.target.value); setDirty(true); }}
          placeholder={t("loop.descPlaceholder")}
        />
        <div className="loop-editor-toolbar-right">
          <button
            className="loop-icon-btn"
            title={`${t("loop.undo")} (⌘Z)`}
            onClick={undo}
          >
            <LucideIcons.Undo2 size={16} />
          </button>
          <button
            className="loop-icon-btn"
            title={`${t("loop.redo")} (⌘⇧Z)`}
            onClick={redo}
          >
            <LucideIcons.Redo2 size={16} />
          </button>
          <span className="loop-editor-toolbar-sep" />
          <button
            className={`loop-icon-btn${showAiAssistant ? " is-active" : ""}`}
            title={t("loop.aiAssistant")}
            onClick={() => {
              setShowAiAssistant((v) => !v);
              if (!showAiAssistant) {
                setSelectedNodeId(null);
                setShowHistory(false);
              }
            }}
          >
            <LucideIcons.Sparkles size={16} />
          </button>
          <button
            className={`loop-icon-btn${showVarsPanel ? " is-active" : ""}`}
            title={t("loop.variables")}
            onClick={() => setShowVarsPanel((v) => !v)}
          >
            <LucideIcons.Variable size={16} />
          </button>
          <button
            className="loop-icon-btn"
            title={t("loop.autoLayout")}
            onClick={handleAutoLayout}
          >
            <LucideIcons.LayoutGrid size={16} />
          </button>
          <button
            className="loop-icon-btn"
            title={t("loop.fitView")}
            onClick={() => reactFlowInstance.fitView({ padding: 0.15, duration: 300 })}
          >
            <LucideIcons.Maximize size={16} />
          </button>
          <button
            className="loop-icon-btn"
            title={t("loop.shortcuts")}
            onClick={() => setShowShortcuts((v) => !v)}
          >
            <LucideIcons.Keyboard size={16} />
          </button>
          <button
            className={`loop-icon-btn${fullscreen ? " is-active" : ""}`}
            title={fullscreen ? t("loop.exitFullscreen") : t("loop.fullscreen")}
            onClick={() => setFullscreen((v) => !v)}
          >
            {fullscreen ? <LucideIcons.Minimize2 size={16} /> : <LucideIcons.Maximize2 size={16} />}
          </button>
          <button
            className={`loop-icon-btn${showHistory ? " is-active" : ""}`}
            title={t("loop.history")}
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
            <span>{saving ? t("loop.saving") : t("loop.save")}</span>
          </button>
          <button
            className="loop-btn loop-btn--primary"
            disabled={running}
            onClick={async () => {
              if (!workflow || running) return;
              // 运行前校验所有节点
              const allErrors: string[] = [];
              for (const n of nodes) {
                const d = n.data as Record<string, unknown>;
                const nt = d.nodeType as NodeType;
                const cfg = (d.config as Record<string, unknown>) ?? {};
                const errs = validateNodeConfig(nt, cfg);
                if (errs.length > 0) {
                  allErrors.push(`${d.label}: ${errs.join("、")}`);
                }
              }
              if (allErrors.length > 0) {
                showToast(`${allErrors.length} 个节点有未填写的必填字段:\n${allErrors.join("\n")}`, { tone: "error" });
                return;
              }
              await handleSave();
              setRunning(true);
              try {
                const result = await invoke<{ run_id: string; status: string; steps_executed: number }>("run_loop", { id: workflow.id });
                showToast(t("loop.runComplete").replace("{count}", String(result.steps_executed)), { tone: "success" });
                setShowHistory(true);
              } catch (e) {
                showToast(String(e), { tone: "error" });
              } finally {
                setRunning(false);
              }
            }}
          >
            {running ? <LucideIcons.Loader2 size={14} className="loop-spin" /> : <Play size={14} />}
            <span>{running ? t("loop.running") : t("loop.run")}</span>
          </button>
        </div>
      </div>

      <div className="loop-editor-body">
        {/* ── Left: Node palette ── */}
        {!paletteOpen && (
          <button
            className="loop-collapse-toggle loop-collapse-toggle--left"
            onClick={() => setPaletteOpen(true)}
            title={t("loop.expandPalette")}
          >
            <LucideIcons.ChevronRight size={14} />
          </button>
        )}
        {paletteOpen && (
        <div className="loop-node-palette">
          <div className="loop-palette-title">{t("loop.paletteTitle")}</div>
          <input
            className="loop-palette-search"
            placeholder={t("loop.searchNodes")}
            value={paletteSearch}
            onChange={(e) => setPaletteSearch(e.target.value)}
          />
          {NODE_CATEGORIES.map((cat) => {
            const pq = paletteSearch.trim().toLowerCase();
            const items = getNodesByCategory(cat.key).filter(
              (m) => !pq || m.label.toLowerCase().includes(pq) || m.labelEn.toLowerCase().includes(pq) || m.type.includes(pq),
            );
            if (pq && items.length === 0) return null;
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
                        {t("loop.presetEmpty")}
                      </div>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
        )}
        {paletteOpen && (
          <button
            className="loop-collapse-toggle loop-collapse-toggle--palette-close"
            onClick={() => setPaletteOpen(false)}
            title={t("loop.collapsePalette")}
          >
            <LucideIcons.ChevronLeft size={14} />
          </button>
        )}

        {/* ── Center: Canvas ── */}
        <div className="loop-canvas-container" ref={reactFlowWrapper}>
          <ReactFlow
            nodes={nodesWithCallbacks}
            edges={edges}
            onNodesChange={(changes) => {
              const hasStructural = changes.some((c) => c.type === "remove" || c.type === "add");
              if (hasStructural) pushSnapshot();
              onNodesChange(changes);
              setDirty(true);
            }}
            onEdgesChange={(changes) => {
              const hasStructural = changes.some((c) => c.type === "remove" || c.type === "add");
              if (hasStructural) pushSnapshot();
              onEdgesChange(changes);
              setDirty(true);
            }}
            onNodeDragStart={() => pushSnapshot()}
            onConnect={onConnect}
            onConnectStart={onConnectStart}
            onConnectEnd={onConnectEnd}
            onNodeClick={(_, node) => setSelectedNodeId(node.id)}
            onNodeDoubleClick={(_, node) => setSelectedNodeId(node.id)}
            onPaneClick={() => { setSelectedNodeId(null); setConnectDrop(null); }}
            nodeTypes={nodeTypes}
            defaultEdgeOptions={{ type: "smoothstep", animated: true }}
            connectionLineType={ConnectionLineType.SmoothStep}
            snapToGrid
            snapGrid={[20, 20]}
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
          {showShortcuts && (
            <div className="loop-shortcuts-overlay" onClick={() => setShowShortcuts(false)}>
              <div className="loop-shortcuts-panel" onClick={(e) => e.stopPropagation()}>
                <div className="loop-shortcuts-title">{t("loop.shortcuts")}</div>
                {[
                  ["⌘Z", t("loop.undo")],
                  ["⌘⇧Z", t("loop.redo")],
                  ["⌘A", t("loop.shortcutSelectAll")],
                  ["⌘C", t("loop.shortcutCopy")],
                  ["⌘V", t("loop.shortcutPaste")],
                  ["⌘S", t("loop.save")],
                  ["Delete", t("loop.delete")],
                ].map(([key, label]) => (
                  <div key={key} className="loop-shortcuts-row">
                    <kbd>{key}</kbd>
                    <span>{label}</span>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>

        {/* ── Right: Config panel toggle ── */}
        {selectedNode && (
          <button
            className="loop-collapse-toggle loop-collapse-toggle--right"
            onClick={() => setSelectedNodeId(null)}
            title={t("loop.collapseConfig")}
          >
            <LucideIcons.ChevronRight size={14} />
          </button>
        )}

        {/* ── Right: Config panel ── */}
        {selectedNode && (
          <LoopConfigPanel
            nodeId={selectedNode.id}
            nodeType={(selectedNode.data as Record<string, unknown>).nodeType as NodeType}
            label={(selectedNode.data as Record<string, unknown>).label as string}
            config={((selectedNode.data as Record<string, unknown>).config as Record<string, unknown>) ?? {}}
            disabled={!!((selectedNode.data as Record<string, unknown>).disabled)}
            workflowId={workflowId}
            upstreamOutputs={selectedUpstreamOutputs}
            aiProviderId={(variables.__ai_provider_id as string) ?? ""}
            aiModel={(variables.__ai_model as string) ?? ""}
            onAiProviderChange={(pid, m) => {
              setVariables((v) => ({ ...v, __ai_provider_id: pid, __ai_model: m }));
              setDirty(true);
            }}
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

        {/* ── Right: Variables panel ── */}
        {showVarsPanel && (
          <div className="loop-vars-panel">
            <div className="loop-vars-panel-header">
              <span className="loop-vars-panel-title">{t("loop.variables")}</span>
              <button className="loop-icon-btn" onClick={() => setShowVarsPanel(false)} title={t("loop.close")}>
                <LucideIcons.X size={14} />
              </button>
            </div>
            <div className="loop-vars-panel-body">
              {Object.entries(variables).map(([key, val]) => (
                <div key={key} className="loop-vars-row">
                  <input
                    className="loop-config-input loop-vars-key"
                    value={key}
                    onChange={(e) => {
                      const next = { ...variables };
                      const v = next[key];
                      delete next[key];
                      next[e.target.value] = v;
                      setVariables(next);
                      setDirty(true);
                    }}
                    placeholder={t("loop.varName")}
                  />
                  <input
                    className="loop-config-input loop-vars-val"
                    value={typeof val === "string" ? val : JSON.stringify(val)}
                    onChange={(e) => {
                      setVariables({ ...variables, [key]: e.target.value });
                      setDirty(true);
                    }}
                    placeholder={t("loop.varValue")}
                  />
                  <button
                    className="loop-icon-btn loop-icon-btn--danger"
                    onClick={() => {
                      const next = { ...variables };
                      delete next[key];
                      setVariables(next);
                      setDirty(true);
                    }}
                    title={t("loop.delete")}
                  >
                    <LucideIcons.Trash2 size={12} />
                  </button>
                </div>
              ))}
              <button
                className="loop-btn loop-btn--secondary loop-btn--sm"
                onClick={() => {
                  const key = `var_${Object.keys(variables).length + 1}`;
                  setVariables({ ...variables, [key]: "" });
                  setDirty(true);
                }}
              >
                <LucideIcons.Plus size={12} />
                <span>{t("loop.addVariable")}</span>
              </button>
            </div>
            <div className="loop-vars-panel-hint">
              {t("loop.variablesHint")}
            </div>
          </div>
        )}

        {/* ── Right: AI Assistant ── */}
        {showAiAssistant && (
          <LoopAiAssistant
            currentNodes={nodes.map((n) => ({
              id: n.id,
              node_type: ((n.data as Record<string, unknown>).nodeType as string) ?? "",
              label: ((n.data as Record<string, unknown>).label as string) ?? "",
              config: ((n.data as Record<string, unknown>).config as Record<string, unknown>) ?? {},
            }))}
            onApply={(genNodes, genEdges) => {
              const dtoNodes = genNodes.map((n) => ({
                id: n.id,
                node_type: n.node_type as NodeType,
                label: n.label,
                config: n.config,
                position: { x: 0, y: 0 },
                disabled: false,
              }));
              const dtoEdges = genEdges.map((e) => ({
                id: `${e.source}-${e.target}`,
                source: e.source,
                source_handle: e.source_handle ?? null,
                target: e.target,
                target_handle: null,
              }));
              const positions = layoutNodes(dtoNodes, dtoEdges);

              const newRfNodes = genNodes.map((n) => {
                const meta = NODE_REGISTRY.find((m) => m.type === n.node_type) ?? NODE_REGISTRY[0];
                const pos = positions.get(n.id) ?? { x: 0, y: 0 };
                return {
                  id: n.id,
                  type: rfNodeType(n.node_type),
                  position: pos,
                  data: {
                    label: n.label,
                    meta,
                    config: n.config,
                    nodeType: n.node_type as NodeType,
                    disabled: false,
                  },
                };
              });

              const newRfEdges = genEdges.map((e, i) => ({
                id: `ai-edge-${i}`,
                source: e.source,
                sourceHandle: e.source_handle ?? undefined,
                target: e.target,
                targetHandle: undefined,
                animated: true,
              }));

              setNodes(newRfNodes);
              setEdges(newRfEdges);
              setDirty(true);
              setShowAiAssistant(false);
              setTimeout(() => reactFlowInstance.fitView({ padding: 0.15, duration: 300 }), 100);
            }}
            onClose={() => setShowAiAssistant(false)}
          />
        )}
      </div>

      <LucideIconPicker
        open={iconPickerOpen}
        selectedId={iconData?.id}
        initialPaint={iconData?.paint}
        initialStyle={iconData?.style}
        toneStyle={toneStyleFromElement(editorRef.current)}
        onClose={() => setIconPickerOpen(false)}
        onSelect={(icon, paint, style) => {
          setIconData({ id: icon.id, paint, style });
          setIconPickerOpen(false);
          setDirty(true);
        }}
      />

      {/* ── Connection-drop node picker ── */}
      {connectDrop && (
        <LoopNodePicker
          x={connectDrop.x}
          y={connectDrop.y}
          savedLoops={savedLoops}
          onSelect={handleNodePickerSelect}
          onClose={() => setConnectDrop(null)}
        />
      )}

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
      {toastHost}
    </div>
  );
}
