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
import type { LoopDto } from "./loopTypes";
import { parseLoopIcon } from "./loopTypes";
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

  const changeView = (v: LoopView) => {
    setViewMode(v);
    localStorage.setItem("astro.loop.viewMode", v);
  };

  const refresh = useCallback(async () => {
    try {
      const list = await invoke<LoopDto[]>("list_loops");
      setLoops(list);
      const runs: Record<string, { status: string; time: string } | null> = {};
      for (const lp of list) {
        try {
          const res = await invoke<{ runs: { status: string; started_at: string }[] }>("list_loop_runs", { workflowId: lp.id, limit: 1 });
          runs[lp.id] = res.runs.length > 0 ? { status: res.runs[0].status, time: res.runs[0].started_at } : null;
        } catch { runs[lp.id] = null; }
      }
      setLastRuns(runs);
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
      const blob = new Blob([json], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `loop-${id}.json`;
      a.click();
      URL.revokeObjectURL(url);
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
            <span>{t("loop.lastRun")}: {lastRuns[lp.id]?.time ?? t("loop.lastRunNever")}</span>
          </div>
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
              {t("loop.nodeCount").replace("{count}", String(lp.nodes.length))} · {t("loop.lastRun")}: {lastRuns[lp.id]?.time ?? t("loop.lastRunNever")}
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
                <div>
                  <h3 className="loop-detail-panel-title">{selectedDetail.name}</h3>
                  <span className={`loop-detail-panel-badge${lastRuns[selectedDetail.id]?.status === "running" ? " is-running" : ""}`}>
                    {lastRuns[selectedDetail.id]?.status === "running" ? t("loop.statusRunning") : t("loop.statusIdle")}
                  </span>
                </div>
                {renderCardActions(selectedDetail)}
              </div>
              <div className="loop-detail-panel-grid">
                <div className="loop-detail-panel-cell">
                  <span className="loop-detail-panel-cell-label">{t("loop.nodes")}</span>
                  <span className="loop-detail-panel-cell-value">{selectedDetail.nodes.length}</span>
                </div>
                <div className="loop-detail-panel-cell">
                  <span className="loop-detail-panel-cell-label">{t("loop.lastRun")}</span>
                  <span className="loop-detail-panel-cell-value">{lastRuns[selectedDetail.id]?.time ?? t("loop.lastRunNever")}</span>
                </div>
                <div className="loop-detail-panel-cell">
                  <span className="loop-detail-panel-cell-label">{t("loop.enabled")}</span>
                  <span className="loop-detail-panel-cell-value">{selectedDetail.enabled ? t("loop.enabledYes") : t("loop.enabledNo")}</span>
                </div>
                <div className="loop-detail-panel-cell">
                  <span className="loop-detail-panel-cell-label">{t("loop.aiCallable")}</span>
                  <span className="loop-detail-panel-cell-value">{selectedDetail.ai_callable ? t("loop.yes") : t("loop.no")}</span>
                </div>
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
          <button className="loop-btn loop-btn--primary" onClick={handleCreate}>
            <Plus size={14} />
            <span>{t("loop.create")}</span>
          </button>
        </div>
      </div>

      {/* ── Content ── */}
      <div className="loop-content">
        {loading && <div className="loop-empty">{t("loop.loading")}</div>}
        {!loading && filtered.length === 0 && (
          <EmptyIllustration
            scene="loop"
            className="loop-empty-illust"
            title={search.trim() ? t("loop.emptySearch") : t("loop.emptyTitle")}
            hint={search.trim() ? undefined : t("loop.emptyHint")}
          />
        )}
        {!loading && filtered.length > 0 && viewMode === "gallery" && renderGallery()}
        {!loading && filtered.length > 0 && viewMode === "list" && renderList()}
        {!loading && filtered.length > 0 && viewMode === "detail" && renderDetail()}
      </div>
      {toastHost}
    </div>
  );
}
