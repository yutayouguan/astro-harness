import { useCallback, useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import {
  Background,
  Controls,
  Handle,
  Position,
  ReactFlow,
  type NodeTypes,
  type NodeProps,
} from "@xyflow/react";
import {
  Bot,
  ExternalLink,
  GitBranch,
  Loader2,
  Maximize2,
  MessageSquare,
  Minimize2,
  Pencil,
  RefreshCw,
  Route,
  Sparkles,
  X,
} from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import { buildBranchFlow, type BranchFlowNode } from "../../lib/chat/branchGraph";
import {
  dispatchSessionsChanged,
  subscribeSessionsChanged,
} from "../../lib/chat/sessionManagement";
import type { BranchGraphDto, BranchGraphNodeDto } from "../../types";

type Props = {
  sessionId: string | null;
  streaming: boolean;
  onOpenSession: (sessionId: string) => void | Promise<void>;
  onOpenSideSession: (sessionId: string) => void | Promise<void>;
  onPrefillInput?: (text: string) => void;
};

/** Codex `last_turn_id` / `before_turn_id` 两种分叉边界。 */
type ForkBoundary = "through_turn" | "before_turn";
type ForkOperation = ForkBoundary | "side";

function BranchNode({ data, selected }: NodeProps<BranchFlowNode>) {
  const { t } = useI18n();
  const node = data.node;
  const preview = node.preview || (node.kind === "branchHead"
    ? node.isEphemeral
      ? t("chat.branches.sideReady")
      : node.status === "legacy"
      ? t("chat.branches.legacyBoundary")
      : t("chat.branches.branchReady")
    : "");
  return (
    <article
      className={[
        "branch-graph-node",
        `is-${node.kind}`,
        node.isEphemeral ? "is-ephemeral" : "",
        node.isCurrent ? "is-current" : "",
        selected ? "is-selected" : "",
      ].filter(Boolean).join(" ")}
    >
      <Handle type="target" position={Position.Top} />
      <div className="branch-node-heading">
        <span className="branch-node-kind" aria-hidden>
          {node.kind === "agent"
            ? <Bot size={12} />
            : node.isEphemeral
              ? <MessageSquare size={12} />
              : node.kind === "branchHead"
                ? <GitBranch size={12} />
                : <Route size={12} />}
        </span>
        <strong>{node.title}</strong>
        {node.isCurrent && <span className="branch-node-current">{t("chat.branches.current")}</span>}
      </div>
      {preview && <p>{preview}</p>}
      <div className="branch-node-meta">
        <span className={`branch-node-status is-${node.status}`}>{node.status}</span>
        {node.model && <span title={node.model}>{node.model}</span>}
        {node.createdAt && <time>{new Date(node.createdAt).toLocaleString([], { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}</time>}
      </div>
      <Handle type="source" position={Position.Bottom} />
    </article>
  );
}

const nodeTypes = { branchNode: BranchNode } satisfies NodeTypes;

export default function BranchGraphPanel({
  sessionId,
  streaming,
  onOpenSession,
  onOpenSideSession,
  onPrefillInput,
}: Props) {
  const { t } = useI18n();
  const [graph, setGraph] = useState<BranchGraphDto | null>(null);
  const [selected, setSelected] = useState<BranchGraphNodeDto | null>(null);
  const [loading, setLoading] = useState(false);
  const [forking, setForking] = useState<ForkOperation | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [fullscreen, setFullscreen] = useState(false);
  const [showAgents, setShowAgents] = useState(true);

  const load = useCallback(async () => {
    if (!sessionId) {
      setGraph(null);
      setSelected(null);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<BranchGraphDto>("get_chat_branch_graph", { sessionId });
      setGraph(next);
      setSelected((current) =>
        current ? next.nodes.find((node) => node.id === current.id) ?? null : null,
      );
    } catch (reason) {
      setError(String(reason));
      setGraph(null);
    } finally {
      setLoading(false);
    }
  }, [sessionId]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => subscribeSessionsChanged(() => { void load(); }), [load]);

  useEffect(() => {
    if (!fullscreen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setFullscreen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [fullscreen]);

  const visibleGraph = useMemo<BranchGraphDto | null>(() => {
    if (!graph || showAgents) return graph;
    return { ...graph, nodes: graph.nodes.filter((node) => node.kind !== "agent") };
  }, [graph, showAgents]);
  const flow = useMemo(
    () => visibleGraph ? buildBranchFlow(visibleGraph) : { nodes: [], edges: [] },
    [visibleGraph],
  );

  const forkSelected = useCallback(
    async (operation: ForkOperation) => {
      if (!selected || selected.kind !== "turn" || forking) return;
      if (operation !== "before_turn" && !selected.canFork) return;
      setForking(operation);
      setError(null);
      try {
        const ephemeral = operation === "side";
        const boundary: ForkBoundary =
          operation === "before_turn" ? "before_turn" : "through_turn";
        const newId = await invoke<string>("fork_chat_session", {
          sourceSessionId: selected.sessionId,
          sourceMessageId: selected.sourceMessageId,
          boundary,
          ephemeral,
          excludeTurns: ephemeral,
          keepChatBubbles: null,
          newSessionId: null,
        });
        dispatchSessionsChanged();
        await (operation === "side"
          ? onOpenSideSession(newId)
          : onOpenSession(newId));
        // before_turn 会把这轮输入留在新分支之外，回填让用户直接改写重发。
        if (operation === "before_turn" && selected.userMessage) {
          onPrefillInput?.(selected.userMessage);
        }
        setFullscreen(false);
      } catch (reason) {
        setError(String(reason));
      } finally {
        setForking(null);
      }
    },
    [forking, onOpenSession, onOpenSideSession, onPrefillInput, selected],
  );

  const canvas = (
    <section className={`branch-graph-panel${fullscreen ? " is-fullscreen" : ""}`}>
      <header className="branch-graph-toolbar">
        <div>
          <strong>{t("chat.branches.title")}</strong>
          <span>
            {t("chat.branches.summary", {
              branches: String(graph?.branchCount ?? 0),
              sides: String(graph?.sideCount ?? 0),
              turns: String(graph?.turnCount ?? 0),
              agents: String(graph?.agentCount ?? 0),
            })}
          </span>
        </div>
        <div className="branch-graph-actions">
          <button
            type="button"
            className={showAgents ? "is-active" : ""}
            onClick={() => setShowAgents((value) => !value)}
            title={t("chat.branches.toggleAgents")}
            aria-pressed={showAgents}
          >
            <Bot size={14} aria-hidden />
          </button>
          <button type="button" onClick={() => void load()} title={t("chat.branches.refresh")}>
            <RefreshCw size={14} aria-hidden />
          </button>
          <button
            type="button"
            onClick={() => setFullscreen((value) => !value)}
            title={fullscreen ? t("chat.branches.exitFullscreen") : t("chat.branches.fullscreen")}
          >
            {fullscreen ? <Minimize2 size={14} aria-hidden /> : <Maximize2 size={14} aria-hidden />}
          </button>
          {fullscreen && (
            <button type="button" onClick={() => setFullscreen(false)} title={t("chat.branches.close")}>
              <X size={14} aria-hidden />
            </button>
          )}
        </div>
      </header>

      <div className="branch-graph-legend" aria-label={t("chat.branches.legend")}>
        <span><i className="is-current" />{t("chat.branches.current")}</span>
        <span><i className="is-fork" />{t("chat.branches.fork")}</span>
        <span><i className="is-side" />{t("chat.branches.side")}</span>
        <span><i className="is-agent" />{t("chat.branches.agent")}</span>
      </div>

      <div className="branch-graph-canvas">
        {loading ? (
          <div className="branch-graph-state"><Loader2 className="is-spinning" aria-hidden />{t("chat.branches.loading")}</div>
        ) : !sessionId ? (
          <div className="branch-graph-state"><GitBranch aria-hidden />{t("chat.branches.noSession")}</div>
        ) : error && !graph ? (
          <div className="branch-graph-state is-error"><X aria-hidden />{error}</div>
        ) : flow.nodes.length === 0 ? (
          <div className="branch-graph-state"><Sparkles aria-hidden />{t("chat.branches.empty")}</div>
        ) : (
          <ReactFlow
            nodes={flow.nodes}
            edges={flow.edges}
            nodeTypes={nodeTypes}
            fitView
            minZoom={0.28}
            maxZoom={1.5}
            nodesDraggable={false}
            nodesConnectable={false}
            onNodeClick={(_, node) => setSelected(node.data.node)}
            proOptions={{ hideAttribution: true }}
          >
            <Background gap={22} size={1} />
            <Controls showInteractive={false} />
          </ReactFlow>
        )}
      </div>

      {selected && (
        <footer className="branch-graph-inspector">
          <div>
            <strong>{selected.title}</strong>
            <span>{selected.preview || selected.agentPath || selected.sessionId}</span>
          </div>
          <div>
            {selected.kind !== "agent" && selected.sessionId !== sessionId && (
              <button
                type="button"
                onClick={() => void (
                  selected.isEphemeral
                    ? onOpenSideSession(selected.sessionId)
                    : onOpenSession(selected.sessionId)
                )}
              >
                <ExternalLink size={13} aria-hidden />{t("chat.branches.openSession")}
              </button>
            )}
            {selected.kind === "turn" && (
              <>
                <button
                  type="button"
                  disabled={streaming || forking !== null}
                  title={t("chat.branches.branchBeforeHint")}
                  onClick={() => void forkSelected("before_turn")}
                >
                  {forking === "before_turn"
                    ? <Loader2 size={13} className="is-spinning" aria-hidden />
                    : <Pencil size={13} aria-hidden />}
                  {t("chat.branches.branchBefore")}
                </button>
                <button
                  type="button"
                  className="is-side"
                  disabled={!selected.canFork || streaming || forking !== null}
                  title={t("chat.branches.sideHint")}
                  onClick={() => void forkSelected("side")}
                >
                  {forking === "side"
                    ? <Loader2 size={13} className="is-spinning" aria-hidden />
                    : <MessageSquare size={13} aria-hidden />}
                  {t("chat.branches.startSide")}
                </button>
                <button
                  type="button"
                  className="is-primary"
                  disabled={!selected.canFork || streaming || forking !== null}
                  onClick={() => void forkSelected("through_turn")}
                >
                  {forking === "through_turn"
                    ? <Loader2 size={13} className="is-spinning" aria-hidden />
                    : <GitBranch size={13} aria-hidden />}
                  {t("chat.branches.branchHere")}
                </button>
              </>
            )}
          </div>
        </footer>
      )}
      {error && graph && <div className="branch-graph-inline-error">{error}</div>}
    </section>
  );

  return fullscreen ? createPortal(canvas, document.body) : canvas;
}
