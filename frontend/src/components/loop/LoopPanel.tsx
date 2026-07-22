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
import LoopEditor from "./LoopEditor";
import ExpandableSearch from "../ui/ExpandableSearch";
import { useI18n } from "../../i18n/LocaleContext";

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
}

export default function LoopPanel({ active, providers }: Props) {
  const { t } = useI18n();
  const [loops, setLoops] = useState<LoopDto[]>([]);
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
    } catch (e) {
      console.error("list_loops failed", e);
    } finally {
      setLoading(false);
    }
  }, []);

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
        name: "未命名创建loop",
        description: "",
      });
      setEditingId(created.id);
      setIsEditing(true);
      void refresh();
    } catch (e) {
      console.error("create_loop failed", e);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await invoke("delete_loop", { id });
      if (selectedDetailId === id) setSelectedDetailId(null);
      void refresh();
    } catch (e) {
      console.error("delete_loop failed", e);
    }
  };

  const handleToggleEnabled = async (id: string, enabled: boolean) => {
    try {
      await invoke("set_loop_enabled", { id, enabled });
      void refresh();
    } catch (e) {
      console.error("set_loop_enabled failed", e);
    }
  };

  const handleToggleAiCallable = async (id: string, callable: boolean) => {
    try {
      await invoke("set_loop_ai_callable", { id, callable });
      void refresh();
    } catch (e) {
      console.error("set_loop_ai_callable failed", e);
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
      console.error("export_loop failed", e);
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
        await invoke("import_loop", { json: text });
        void refresh();
      } catch (e) {
        console.error("import_loop failed", e);
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
        title="运行一次"
        onClick={async () => {
          try {
            const result = await invoke<{ run_id: string; status: string; steps_executed: number }>("run_loop", { id: lp.id });
            console.log("run_loop result:", result);
          } catch (e) {
            console.error("run_loop failed", e);
          }
        }}
      >
        <Play size={14} />
      </button>
      <button
        className="loop-icon-btn"
        title="编辑"
        onClick={() => {
          setEditingId(lp.id);
          setIsEditing(true);
        }}
      >
        <Pencil size={14} />
      </button>
      <button
        className="loop-icon-btn"
        title="导出"
        onClick={() => void handleExport(lp.id)}
      >
        <Download size={14} />
      </button>
      <button
        className="loop-icon-btn loop-icon-btn--danger"
        title="删除"
        onClick={() => void handleDelete(lp.id)}
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
              <Bot size={18} />
            </div>
            <div className="loop-card-info">
              <span className="loop-card-name">{lp.name}</span>
              <span className="loop-card-badge">空闲</span>
            </div>
            {renderCardActions(lp)}
          </div>
          <div className="loop-card-meta">
            <span>{lp.nodes.length} 个节点</span>
            <span>·</span>
            <span>上次运行: 从未</span>
          </div>
          <div className="loop-card-toggles">
            <label className="loop-toggle">
              <input
                type="checkbox"
                checked={lp.enabled}
                onChange={(e) => void handleToggleEnabled(lp.id, e.target.checked)}
              />
              <Power size={12} />
              <span>启用</span>
            </label>
            <label className="loop-toggle">
              <input
                type="checkbox"
                checked={lp.ai_callable}
                onChange={(e) => void handleToggleAiCallable(lp.id, e.target.checked)}
              />
              <Bot size={12} />
              <span>AI 可调用</span>
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
            <Bot size={16} />
          </div>
          <div className="loop-list-row-body">
            <span className="loop-list-row-name">{lp.name}</span>
            <span className="loop-list-row-meta">
              {lp.nodes.length} 个节点 · 上次运行: 从未
            </span>
          </div>
          <div className="loop-list-row-toggles">
            <label className="loop-toggle loop-toggle--compact">
              <input
                type="checkbox"
                checked={lp.enabled}
                onChange={(e) => void handleToggleEnabled(lp.id, e.target.checked)}
              />
              <span>启用</span>
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
            {lp.enabled ? "已启用" : "未启用"}
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
              <Bot size={14} />
            </div>
            <div className="loop-detail-sidebar-text">
              <span className="loop-detail-sidebar-name">{lp.name}</span>
              <span className="loop-detail-sidebar-meta">{lp.nodes.length} 个节点</span>
            </div>
            <span className={`loop-detail-sidebar-dot${lp.enabled ? " is-active" : ""}`} />
          </button>
        ))}
      </div>
      <div className="loop-detail-panel">
        {selectedDetail ? (
          <>
            <div className="loop-detail-panel-header">
              <div className="loop-detail-panel-icon">
                <Bot size={22} />
              </div>
              <div>
                <h3 className="loop-detail-panel-title">{selectedDetail.name}</h3>
                <span className="loop-detail-panel-badge">空闲</span>
              </div>
              {renderCardActions(selectedDetail)}
            </div>
            <div className="loop-detail-panel-grid">
              <div className="loop-detail-panel-cell">
                <span className="loop-detail-panel-cell-label">节点数</span>
                <span className="loop-detail-panel-cell-value">{selectedDetail.nodes.length}</span>
              </div>
              <div className="loop-detail-panel-cell">
                <span className="loop-detail-panel-cell-label">上次运行</span>
                <span className="loop-detail-panel-cell-value">从未</span>
              </div>
              <div className="loop-detail-panel-cell">
                <span className="loop-detail-panel-cell-label">状态</span>
                <span className="loop-detail-panel-cell-value">{selectedDetail.enabled ? "已启用" : "未启用"}</span>
              </div>
              <div className="loop-detail-panel-cell">
                <span className="loop-detail-panel-cell-label">AI 可调用</span>
                <span className="loop-detail-panel-cell-value">{selectedDetail.ai_callable ? "是" : "否"}</span>
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
                <span>启用</span>
              </label>
              <label className="loop-toggle">
                <input
                  type="checkbox"
                  checked={selectedDetail.ai_callable}
                  onChange={(e) => void handleToggleAiCallable(selectedDetail.id, e.target.checked)}
                />
                <Bot size={12} />
                <span>AI 可调用</span>
              </label>
            </div>
          </>
        ) : (
          <div className="loop-detail-panel-empty">
            选择一个 Loop 查看详情
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
      {/* ── Header ── */}
      <div className="loop-panel-header">
        <div className="loop-panel-title-area">
          <h2 className="loop-panel-title">Loop</h2>
          <p className="loop-panel-desc">
            用节点搭好一条自动化流程：手动运行、按定时 / Webhook
            自动触发，或让 code 模式的智能体直接调用。每条流程下方有「启用」和「AI
            可调用」两个开关。
          </p>
        </div>
      </div>

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
            <span>导入</span>
          </button>
          <button className="loop-btn loop-btn--primary" onClick={handleCreate}>
            <Plus size={14} />
            <span>新建</span>
          </button>
        </div>
      </div>

      {/* ── Content ── */}
      <div className="loop-content">
        {loading && <div className="loop-empty">加载中…</div>}
        {!loading && filtered.length === 0 && (
          <div className="loop-empty">
            {search.trim() ? (
              <p>未找到匹配的 Loop</p>
            ) : (
              <>
                <p>暂无 Loop 工作流</p>
                <p className="loop-empty-hint">
                  点击「新建」开始搭建你的第一条自动化流程
                </p>
              </>
            )}
          </div>
        )}
        {!loading && filtered.length > 0 && viewMode === "gallery" && renderGallery()}
        {!loading && filtered.length > 0 && viewMode === "list" && renderList()}
        {!loading && filtered.length > 0 && viewMode === "detail" && renderDetail()}
      </div>
    </div>
  );
}
