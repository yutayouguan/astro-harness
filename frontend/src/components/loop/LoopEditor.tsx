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
import LoopRunHistory from "./LoopRunHistory";
import LoopRunDetail from "./LoopRunDetail";
import LoopIcon from "./LoopIcon";
import LoopAiAssistant from "./LoopAiAssistant";
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
  return (
    <div
      className={`loop-rf-node${selected ? " is-selected" : ""}${data.disabled ? " is-disabled" : ""}`}
      style={{ "--node-color": data.meta.color } as React.CSSProperties}
    >
      {selected && (
        <div className="loop-rf-node-toolbar">
          <button
            className="loop-rf-toolbar-btn"
            title={t("loop.run")}
            onClick={(e) => { e.stopPropagation(); data.onRunNode?.(); }}
          >
            <LucideIcons.Play size={12} />
          </button>
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
          <button className="loop-rf-toolbar-btn" title={t("loop.run")}
            onClick={(e) => { e.stopPropagation(); data.onRunNode?.(); }}>
            <LucideIcons.Play size={12} />
          </button>
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
      <div className="loop-rf-branch-handles">
        {branches.map((b, i) => (
          <div key={b.id} className="loop-rf-branch-row">
            <span className="loop-rf-branch-label">{b.label || b.id}</span>
            <Handle
              type="source"
              position={Position.Right}
              id={b.id}
              className="loop-rf-handle"
              style={{ top: `${30 + (i + 1) * (40 / (branches.length + 1))}px` }}
            />
          </div>
        ))}
      </div>
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

  const [nodes, setNodes, onNodesChange] = useNodesState<RFNode>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<RFEdge>([]);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);

  // 已保存的工作流列表（自定义分组使用）
  const [savedLoops, setSavedLoops] = useState<LoopDto[]>([]);

  // run history state
  const [showHistory, setShowHistory] = useState(false);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [showAiAssistant, setShowAiAssistant] = useState(false);

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
          onRunNode: () => showToast(t("loop.runNodeUnsupported"), { tone: "info" }),
          onToggleDisable: () => toggleNodeDisabled(n.id),
          onDeleteNode: async () => {
            const ok = await confirm({ title: t("loop.deleteNode"), message: t("loop.deleteNodeConfirm").replace("{name}", String((n.data as Record<string, unknown>).label)), confirmLabel: t("loop.delete"), variant: "danger" });
            if (ok) deleteNode(n.id);
          },
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
          })),
        );
      } catch (e) {
        showToast(String(e), { tone: "error" });
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
            onClick={async () => {
              if (!workflow) return;
              await handleSave();
              try {
                const result = await invoke<{ run_id: string; status: string; steps_executed: number }>("run_loop", { id: workflow.id });
                showToast(t("loop.runComplete").replace("{count}", String(result.steps_executed)), { tone: "success" });
                setShowHistory(true);
              } catch (e) {
                showToast(String(e), { tone: "error" });
              }
            }}
          >
            <Play size={14} />
            <span>{t("loop.run")}</span>
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
              onNodesChange(changes);
              setDirty(true);
            }}
            onEdgesChange={(changes) => {
              onEdgesChange(changes);
              setDirty(true);
            }}
            onConnect={onConnect}
            onConnectStart={onConnectStart}
            onConnectEnd={onConnectEnd}
            onNodeClick={(_, node) => setSelectedNodeId(node.id)}
            onPaneClick={() => { setSelectedNodeId(null); setConnectDrop(null); }}
            nodeTypes={nodeTypes}
            snapToGrid
            snapGrid={[1, 1]}
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
              // 自动布局：水平分层
              const layoutX = 200;
              const layoutY = 100;
              const gapX = 250;
              const gapY = 120;

              const newRfNodes = genNodes.map((n, i) => {
                const meta = NODE_REGISTRY.find((m) => m.type === n.node_type) ?? NODE_REGISTRY[0];
                return {
                  id: n.id,
                  type: "loopNode" as const,
                  position: { x: layoutX + (i % 4) * gapX, y: layoutY + Math.floor(i / 4) * gapY },
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
                animated: true,
              }));

              setNodes(newRfNodes);
              setEdges(newRfEdges);
              setDirty(true);
              setShowAiAssistant(false);
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
