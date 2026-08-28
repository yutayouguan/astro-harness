import { useCallback, useEffect, useMemo, useState, type SVGProps } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Plus,
  Play,
  Pencil,
  Trash2,
  Download,
  Upload,
  Power,
  Bot,
} from "lucide-react";
import { LOOP_ICON_MAP } from "./loopIcons";
import type { LoopDto } from "./loopTypes";
import { parseLoopIcon } from "./loopTypes";
import { LOOP_TEMPLATES, type LoopTemplate } from "./loopTemplates";
import { layoutNodes } from "./loopLayout";
import LoopIcon from "./LoopIcon";
import LoopEditor from "./LoopEditor";
import LoopPreview from "./LoopPreview";
import ExpandableSearch from "../ui/ExpandableSearch";
import { useI18n } from "../../i18n/LocaleContext";
import { EmptyIllustration } from "../../illustrations";
import { useTransientToast } from "../../hooks/ui/useTransientToast";
import { useConfirm } from "../../hooks/ui/DialogContext";

type LoopView = "gallery" | "list" | "detail";

function readLoopView(): LoopView {
  const v = localStorage.getItem("astro.loop.viewMode");
  if (v === "gallery" || v === "list" || v === "detail") return v;
  return "gallery";
}

// ── View toggle icons (same style as CronPanel) ──────────────────────

function IconViewGallery(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width={15} height={15} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" {...props}>
      <rect x={3} y={3} width={7} height={7} rx={1.5} />
      <rect x={14} y={3} width={7} height={7} rx={1.5} />
      <rect x={3} y={14} width={7} height={7} rx={1.5} />
      <rect x={14} y={14} width={7} height={7} rx={1.5} />
    </svg>
  );
}

function IconViewList(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width={15} height={15} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" {...props}>
      <path d="M8 6h13M8 12h13M8 18h13" />
      <circle cx={4} cy={6} r={1} fill="currentColor" stroke="none" />
      <circle cx={4} cy={12} r={1} fill="currentColor" stroke="none" />
      <circle cx={4} cy={18} r={1} fill="currentColor" stroke="none" />
    </svg>
  );
}

function IconViewDetail(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width={15} height={15} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" {...props}>
      <rect x={3} y={4} width={8} height={16} rx={1.5} />
      <rect x={13} y={4} width={8} height={16} rx={1.5} />
    </svg>
  );
}

interface Props {
  active: boolean;
  providers: { id: string; name: string; model: string; kind: string }[];
  onCollapseSidebar?: () => void;
  onExpandSidebar?: () => void;
}

function formatRelativeTime(iso: string | null | undefined): string {
  if (!iso) return "—";
  const ms = Date.parse(iso);
  if (!Number.isFinite(ms)) return iso;
  const diff = Math.max(0, Date.now() - ms);
  const m = 60_000, h = 60 * m, d = 24 * h;
  if (diff < m) return "刚刚";
  if (diff < h) return `${Math.floor(diff / m)} 分钟前`;
  if (diff < d) return `${Math.floor(diff / h)} 小时前`;
  if (diff < 7 * d) return `${Math.floor(diff / d)} 天前`;
  return new Date(ms).toLocaleDateString();
}

export default function LoopPanel({ active, providers, onCollapseSidebar, onExpandSidebar }: Props) {
  const { t } = useI18n();
  const { showToast, toastHost } = useTransientToast();
  const confirm = useConfirm();
  const [loops, setLoops] = useState<LoopDto[]>([]);
  const [lastRuns, setLastRuns] = useState<Record<string, { status: string; time: string } | null>>({});
  const [loading, setLoading] = useState(true);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [isEditing, setIsEditing] = useState(false);
  const [search, setSearch] = useState("");
  const [viewMode, setViewMode] = useState<LoopView>(readLoopView);
  const [selectedDetailId, setSelectedDetailId] = useState<string | null>(null);
  const [showTemplates, setShowTemplates] = useState(false);

  const changeView = (v: LoopView) => {
    setViewMode(v);
    localStorage.setItem("astro.loop.viewMode", v);
  };

  const refresh = useCallback(async () => {
    try {
      const list = await invoke<LoopDto[]>("list_loops");
      setLoops(list);
      const results = await Promise.all(
        list.map((lp) =>
          invoke<{ status: string; started_at: string }[]>("list_loop_runs", { workflowId: lp.id, limit: 1 })
            .then((runs) => [lp.id, runs.length > 0 ? { status: runs[0].status, time: runs[0].started_at } : null, false] as const)
            .catch(() => [lp.id, null, true] as const)
        ),
      );
      const runs: Record<string, { status: string; time: string } | null> = {};
      let errorCount = 0;
      for (const [id, run, isError] of results) {
        runs[id] = run;
        if (isError) errorCount++;
      }
      setLastRuns(runs);
      if (errorCount > 0 && errorCount === list.length && list.length > 0) {
        showToast(t("loop.runQueryFailed"), { tone: "error" });
      }
    } catch (e) {
      showToast(String(e), { tone: "error" });
    } finally {
      setLoading(false);
    }
  }, [showToast]);

  useEffect(() => {
    if (active) void refresh();
  }, [active, refresh]);

  const filtered = useMemo(() => {
    if (!search.trim()) return loops;
    const q = search.toLowerCase();
    return loops.filter((lp) => lp.name.toLowerCase().includes(q));
  }, [loops, search]);

  const selectedDetail = useMemo(
    () => filtered.find((lp) => lp.id === selectedDetailId) ?? null,
    [filtered, selectedDetailId],
  );

  const handleCreate = async () => {
    try {
      const created = await invoke<LoopDto>("create_loop", {
        name: t("loop.defaultName"),
        description: "",
      });
      setEditingId(created.id);
      setIsEditing(true);
      onCollapseSidebar?.();
      void refresh();
    } catch (e) {
      showToast(String(e), { tone: "error" });
    }
  };

  const handleCreateFromTemplate = async (tpl: LoopTemplate) => {
    try {
      const created = await invoke<LoopDto>("create_loop", {
        name: tpl.name,
        description: tpl.description,
      });
      const dtoNodes = tpl.nodes.map((n) => ({
        ...n,
        position: { x: 0, y: 0 },
        disabled: false,
      }));
      const dtoEdges = tpl.edges.map((e, i) => ({
        id: `tpl-edge-${i}`,
        source: e.source,
        source_handle: e.source_handle ?? null,
        target: e.target,
        target_handle: null,
      }));
      const positions = layoutNodes(dtoNodes, dtoEdges);
      const nodes = dtoNodes.map((n) => {
        const pos = positions.get(n.id) ?? { x: 0, y: 0 };
        return { ...n, position: pos };
      });
      await invoke("save_loop", {
        data: {
          id: created.id,
          name: tpl.name,
          description: tpl.description,
          nodes,
          edges: dtoEdges,
          variables: {},
          icon: null,
          enabled: false,
          ai_callable: false,
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        },
      });
      setEditingId(created.id);
      setIsEditing(true);
      setShowTemplates(false);
      onCollapseSidebar?.();
      void refresh();
    } catch (e) {
      showToast(String(e), { tone: "error" });
    }
  };

  const handleDelete = async (id: string, name: string) => {
    const ok = await confirm({ title: t("loop.deleteTitle"), message: t("loop.deleteConfirm").replace("{name}", name), confirmLabel: t("loop.delete"), variant: "danger" });
    if (!ok) return;
    try {
      await invoke("delete_loop", { id });
      if (selectedDetailId === id) setSelectedDetailId(null);
      showToast(t("loop.deleted"), { tone: "success" });
      void refresh();
    } catch (e) {
      showToast(String(e), { tone: "error" });
    }
  };

  const handleToggleEnabled = async (id: string, enabled: boolean) => {
    try {
      await invoke("set_loop_enabled", { id, enabled });
      void refresh();
    } catch (e) {
      showToast(String(e), { tone: "error" });
    }
  };

  const handleToggleAiCallable = async (id: string, callable: boolean) => {
    try {
      await invoke("set_loop_ai_callable", { id, callable });
      void refresh();
    } catch (e) {
      showToast(String(e), { tone: "error" });
    }
  };

  const handleExport = async (id: string) => {
    try {
      const json = await invoke<string>("export_loop", { id });
      const fileName = `loop-${id}.json`;
      const savedPath = await invoke<string>("export_loop_svg", { path: fileName, content: json });
      showToast(`${t("loop.exported")}: ${savedPath}`, { tone: "success" });
    } catch (e) {
      showToast(String(e), { tone: "error" });
    }
  };

  const handleDuplicate = async (lp: LoopDto) => {
    try {
      const created = await invoke<LoopDto>("create_loop", {
        name: `${lp.name} (${t("loop.copy")})`,
        description: lp.description,
      });
      await invoke("save_loop", {
        data: {
          id: created.id,
          name: created.name,
          description: lp.description,
          nodes: lp.nodes,
          edges: lp.edges,
          variables: lp.variables,
          icon: lp.icon ?? null,
          enabled: lp.enabled,
          ai_callable: lp.ai_callable,
          created_at: lp.created_at,
          updated_at: new Date().toISOString(),
        },
      });
      showToast(t("loop.duplicated"), { tone: "success" });
      void refresh();
    } catch (e) {
      showToast(String(e), { tone: "error" });
    }
  };

  const handleImport = async () => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      const text = await file.text();
      try {
        const parsed = JSON.parse(text);
        const importId = parsed.id;
        if (importId && loops.some((lp) => lp.id === importId)) {
          const ok = await confirm({
            title: t("loop.importConflictTitle"),
            message: t("loop.importConflictMsg"),
            confirmLabel: t("loop.importOverwrite"),
            variant: "danger",
          });
          if (!ok) return;
        }
        await invoke("import_loop", { json: text });
        showToast(t("loop.imported"), { tone: "success" });
        void refresh();
      } catch (e) {
        showToast(String(e), { tone: "error" });
      }
    };
    input.click();
  };

  // ── Editing mode ──
  if (isEditing) {
    return (
      <LoopEditor
        workflowId={editingId}
        providers={providers}
        onBack={() => {
          setIsEditing(false);
          setEditingId(null);
          onExpandSidebar?.();
          void refresh();
        }}
      />
    );
  }

  // ── Shared card actions (used by gallery + detail) ──
  const renderCardActions = (lp: LoopDto) => (
    <div className="loop-card-actions">
      <button
        className="loop-icon-btn"
        title={t("loop.run")}
        onClick={async () => {
          try {
            const result = await invoke<{ run_id: string; status: string; steps_executed: number }>("run_loop", { id: lp.id });
            showToast(t("loop.runComplete").replace("{count}", String(result.steps_executed)), { tone: "success" });
            void refresh();
          } catch (e) {
            showToast(String(e), { tone: "error" });
          }
        }}
      >
        <Play size={14} />
      </button>
      <button
        className="loop-icon-btn"
        title={t("loop.edit")}
        onClick={() => {
          setEditingId(lp.id);
          setIsEditing(true);
          onCollapseSidebar?.();
        }}
      >
        <Pencil size={14} />
      </button>
      <button
        className="loop-icon-btn"
        title={t("loop.duplicate")}
        onClick={() => void handleDuplicate(lp)}
      >
        <LOOP_ICON_MAP.Copy size={14} />
      </button>
      <button
        className="loop-icon-btn"
        title={t("loop.export")}
        onClick={() => void handleExport(lp.id)}
      >
        <Download size={14} />
      </button>
      <button
        className="loop-icon-btn loop-icon-btn--danger"
        title={t("loop.delete")}
        onClick={() => void handleDelete(lp.id, lp.name)}
      >
        <Trash2 size={14} />
      </button>
    </div>
  );

  // ── Gallery view (grid cards) ──
  const renderGallery = () => (
    <div className="loop-gallery">
      {filtered.map((lp) => (
        <article key={lp.id} className="loop-card">
          <div className="loop-card-header">
            <div className="loop-card-icon">
              <LoopIcon icon={parseLoopIcon(lp.icon)} size={18} />
            </div>
            <div className="loop-card-info">
              <span className="loop-card-name">{lp.name}</span>
              <span className={`loop-card-badge${lastRuns[lp.id]?.status === "running" ? " is-running" : ""}`}>
                {lastRuns[lp.id]?.status === "running" ? t("loop.statusRunning") : t("loop.statusIdle")}
              </span>
            </div>
            {renderCardActions(lp)}
          </div>
          <div className="loop-card-meta">
            <span>{t("loop.nodeCount").replace("{count}", String(lp.nodes.length))}</span>
            <span>·</span>
            <span>{t("loop.lastRun")}: {formatRelativeTime(lastRuns[lp.id]?.time) === "—" ? t("loop.lastRunNever") : formatRelativeTime(lastRuns[lp.id]?.time)}</span>
          </div>
          {lp.nodes.length > 0 && (
            <div className="loop-card-preview">
              <LoopPreview workflow={lp} />
            </div>
          )}
          <div className="loop-card-toggles">
            <label className="loop-toggle">
              <input
                type="checkbox"
                checked={lp.enabled}
                onChange={(e) => void handleToggleEnabled(lp.id, e.target.checked)}
              />
              <Power size={12} />
              <span>{t("loop.enabled")}</span>
            </label>
            <label className="loop-toggle">
              <input
                type="checkbox"
                checked={lp.ai_callable}
                onChange={(e) => void handleToggleAiCallable(lp.id, e.target.checked)}
              />
              <Bot size={12} />
              <span>{t("loop.aiCallable")}</span>
            </label>
          </div>
        </article>
      ))}
    </div>
  );

  // ── List view (compact rows) ──
  const renderList = () => (
    <div className="loop-list-view">
      {filtered.map((lp) => (
        <div key={lp.id} className="loop-list-row">
          <div className="loop-list-row-icon">
            <LoopIcon icon={parseLoopIcon(lp.icon)} size={16} />
          </div>
          <div className="loop-list-row-body">
            <span className="loop-list-row-name">{lp.name}</span>
            <span className="loop-list-row-meta">
              {t("loop.nodeCount").replace("{count}", String(lp.nodes.length))} · {t("loop.lastRun")}: {formatRelativeTime(lastRuns[lp.id]?.time) === "—" ? t("loop.lastRunNever") : formatRelativeTime(lastRuns[lp.id]?.time)}
            </span>
          </div>
          <div className="loop-list-row-toggles">
            <label className="loop-toggle loop-toggle--compact">
              <input
                type="checkbox"
                checked={lp.enabled}
                onChange={(e) => void handleToggleEnabled(lp.id, e.target.checked)}
              />
              <span>{t("loop.enabled")}</span>
            </label>
            <label className="loop-toggle loop-toggle--compact">
              <input
                type="checkbox"
                checked={lp.ai_callable}
                onChange={(e) => void handleToggleAiCallable(lp.id, e.target.checked)}
              />
              <span>AI</span>
            </label>
          </div>
          <span className={`loop-list-row-badge${lp.enabled ? " is-active" : ""}`}>
            {lp.enabled ? t("loop.enabledYes") : t("loop.enabledNo")}
          </span>
          {renderCardActions(lp)}
        </div>
      ))}
    </div>
  );

  // ── Detail view (split pane) ──
  const renderDetail = () => (
    <div className="loop-detail">
      <div className="loop-detail-sidebar">
        {filtered.map((lp) => (
          <button
            key={lp.id}
            className={`loop-detail-sidebar-item${selectedDetailId === lp.id ? " is-selected" : ""}`}
            onClick={() => setSelectedDetailId(lp.id)}
          >
            <div className="loop-detail-sidebar-icon">
              <LoopIcon icon={parseLoopIcon(lp.icon)} size={14} />
            </div>
            <div className="loop-detail-sidebar-text">
              <span className="loop-detail-sidebar-name">{lp.name}</span>
              <span className="loop-detail-sidebar-meta">{t("loop.nodeCount").replace("{count}", String(lp.nodes.length))}</span>
            </div>
            <span className={`loop-detail-sidebar-dot${lp.enabled ? " is-active" : ""}`} />
          </button>
        ))}
      </div>
      <div className="loop-detail-panel">
        {selectedDetail ? (
          <>
            <div className="loop-detail-panel-top">
              <div className="loop-detail-panel-header">
                <div className="loop-detail-panel-icon">
                  <LoopIcon icon={parseLoopIcon(selectedDetail.icon)} size={22} />
                </div>
                <div className="loop-detail-panel-title-group">
                  <h3 className="loop-detail-panel-title">{selectedDetail.name}</h3>
                  <span className={`loop-detail-panel-badge${lastRuns[selectedDetail.id]?.status === "running" ? " is-running" : ""}`}>
                    {lastRuns[selectedDetail.id]?.status === "running" ? t("loop.statusRunning") : t("loop.statusIdle")}
                  </span>
                </div>
              </div>
              {renderCardActions(selectedDetail)}
              <div className="loop-detail-panel-stats">
                <span>{t("loop.nodeCount").replace("{count}", String(selectedDetail.nodes.length))}</span>
                <span className="loop-detail-panel-stats-sep">·</span>
                <span>{t("loop.lastRun")}: {formatRelativeTime(lastRuns[selectedDetail.id]?.time) === "—" ? t("loop.lastRunNever") : formatRelativeTime(lastRuns[selectedDetail.id]?.time)}</span>
              </div>
              <div className="loop-detail-panel-toggles">
                <label className="loop-toggle">
                  <input
                    type="checkbox"
                    checked={selectedDetail.enabled}
                    onChange={(e) => void handleToggleEnabled(selectedDetail.id, e.target.checked)}
                  />
                  <Power size={12} />
                  <span>{t("loop.enabled")}</span>
                </label>
                <label className="loop-toggle">
                  <input
                    type="checkbox"
                    checked={selectedDetail.ai_callable}
                    onChange={(e) => void handleToggleAiCallable(selectedDetail.id, e.target.checked)}
                  />
                  <Bot size={12} />
                  <span>{t("loop.aiCallable")}</span>
                </label>
              </div>
            </div>
            {selectedDetail.nodes.length > 0 && (
              <LoopPreview workflow={selectedDetail} />
            )}
          </>
        ) : (
          <div className="loop-detail-panel-empty">
            {t("loop.detailSelect")}
          </div>
        )}
      </div>
    </div>
  );

  const VIEW_OPTIONS: { id: LoopView; Icon: typeof IconViewGallery; labelKey: string }[] = [
    { id: "gallery", Icon: IconViewGallery, labelKey: "loop.view.gallery" },
    { id: "list", Icon: IconViewList, labelKey: "loop.view.list" },
    { id: "detail", Icon: IconViewDetail, labelKey: "loop.view.detail" },
  ];

  return (
    <div className="loop-panel">
      {/* ── Toolbar ── */}
      <div className="loop-toolbar">
        <div className="loop-toolbar-start">
          <div className="loop-create-group">
            <button className="loop-btn loop-btn--primary" onClick={handleCreate}>
              <Plus size={14} />
              <span>{t("loop.create")}</span>
            </button>
            <button
              className="loop-btn loop-btn--primary loop-btn--template"
              onClick={() => setShowTemplates((v) => !v)}
              title={t("loop.templateTitle")}
              aria-expanded={showTemplates}
            >
              <LOOP_ICON_MAP.LayoutTemplate size={14} />
              <span>{t("loop.templateTitle")}</span>
            </button>
          </div>
        </div>
        <div className="loop-toolbar-end">
          <ExpandableSearch
            value={search}
            onChange={setSearch}
            placeholderKey="loop.searchPlaceholder"
          />
          <div className="loop-view-toggle" role="group" aria-label={t("loop.viewMode")}>
            {VIEW_OPTIONS.map(({ id, Icon, labelKey }) => (
              <button
                key={id}
                type="button"
                className={`loop-view-btn${viewMode === id ? " is-active" : ""}`}
                onClick={() => changeView(id)}
                title={t(labelKey as Parameters<typeof t>[0])}
                aria-label={t(labelKey as Parameters<typeof t>[0])}
                aria-pressed={viewMode === id}
              >
                <Icon />
              </button>
            ))}
          </div>
          <button className="loop-btn loop-btn--secondary" onClick={handleImport}>
            <Upload size={14} />
            <span>{t("loop.import")}</span>
          </button>
        </div>
      </div>

      {/* ── Template picker ── */}
      {showTemplates && (
        <div className="loop-template-picker">
          <div className="loop-template-picker-header">
            <span>{t("loop.templateTitle")}</span>
            <button className="loop-icon-btn" onClick={() => setShowTemplates(false)}>
              <LOOP_ICON_MAP.X size={14} />
            </button>
          </div>
          <div className="loop-template-grid">
            {LOOP_TEMPLATES.map((tpl) => {
              const Icon = LOOP_ICON_MAP[tpl.icon];
              return (
                <button
                  key={tpl.id}
                  className="loop-template-card"
                  onClick={() => void handleCreateFromTemplate(tpl)}
                >
                  <span className="loop-template-card-icon">
                    {Icon && <Icon size={20} />}
                  </span>
                  <span className="loop-template-card-name">{tpl.name}</span>
                  <span className="loop-template-card-desc">{tpl.description}</span>
                </button>
              );
            })}
          </div>
        </div>
      )}

      {/* ── Content ── */}
      <div className="loop-content" hidden={showTemplates}>
        {loading && <div className="loop-empty">{t("loop.loading")}</div>}
        {!loading && filtered.length === 0 && (
          <div className="loop-empty-with-templates">
            <EmptyIllustration
              scene="loop"
              className="loop-empty-illust"
              title={search.trim() ? t("loop.emptySearch") : t("loop.emptyTitle")}
              hint={search.trim() ? undefined : t("loop.emptyHint")}
            />
            {!search.trim() && (
              <div className="loop-empty-templates">
                <div className="loop-empty-templates-title">{t("loop.templateQuickStart")}</div>
                <div className="loop-template-grid">
                  {LOOP_TEMPLATES.slice(0, 3).map((tpl) => {
                    const Icon = LOOP_ICON_MAP[tpl.icon];
                    return (
                      <button
                        key={tpl.id}
                        className="loop-template-card"
                        onClick={() => void handleCreateFromTemplate(tpl)}
                      >
                        <span className="loop-template-card-icon">
                          {Icon && <Icon size={20} />}
                        </span>
                        <span className="loop-template-card-name">{tpl.name}</span>
                        <span className="loop-template-card-desc">{tpl.description}</span>
                      </button>
                    );
                  })}
                </div>
              </div>
            )}
          </div>
        )}
        {!loading && filtered.length > 0 && viewMode === "gallery" && renderGallery()}
        {!loading && filtered.length > 0 && viewMode === "list" && renderList()}
        {!loading && filtered.length > 0 && viewMode === "detail" && renderDetail()}
      </div>
      {toastHost}
    </div>
  );
}
