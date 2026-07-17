/** 文件空间：沙箱目录浏览、多选与批量操作。 */
import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ExternalLink,
  History,
  LayoutGrid,
  MessageCircle,
  MoreHorizontal,
  Navigation,
  Palette,
} from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import { useConfirm } from "../../hooks/ui/DialogContext";
import { useTransientToast } from "../../hooks/ui/useTransientToast";
import type { MessageKey } from "../../i18n/messages";
import type {
  ArtifactCategory,
  ArtifactDto,
  ArtifactSessionGroupDto,
  ListArtifactsResult,
} from "../../types";
import {
  IconWsBackChat,
  IconWsFile,
  IconWsFileCode,
  IconWsFileImage,
  IconWsFileSheet,
  IconWsFileSlides,
  IconWsFileText,
  IconWsFileVideo,
  IconWsLayers,
  IconWsViewGrid,
  IconWsViewList,
} from "../workspace/WorkspaceIcons";
import AgentPicker from "../agents/AgentPicker";
import AnimatedSwitch from "../ui/AnimatedSwitch";
import ExpandableSearch from "../ui/ExpandableSearch";
import FileContextMenu, { type FileMenuAction } from "./FileContextMenu";
import FileGlyph from "./FileGlyph";
import FileSpaceBatchBar from "./FileSpaceBatchBar";
import FileSpaceViewer from "./FileSpaceViewer";
import { formatSize, isTauri } from "../../lib/filespace/fileMeta";
import { EmptyIllustration } from "../../illustrations";
import { useFileSelection } from "../../hooks/ui/useFileSelection";
import { useAgentsChanged } from "../../lib/agent/agentsChanged";
import type { AgentInfo } from "../../types/agent";

/** 附件到聊天：新会话或当前会话 */
type AttachMode = "new" | "current";

/** 文件空间面板入参 */
type Props = {
  active: boolean;
  /** 打开产物所属会话（可选定位到 message） */
  onOpenSession: (sessionId: string, messageId?: string | null) => void;
  onClose?: () => void;
  /** 将选中产物作为附件挂到聊天 */
  onAttachFiles?: (files: ArtifactDto[], mode: AttachMode) => void | Promise<void>;
  /** 在工作区（浏览模式）中打开该产物的绝对路径 */
  onOpenInWorkspace?: (path: string) => void;
};

/** 列表 / 网格布局 */
type LayoutMode = "list" | "grid";

const CATEGORIES: ArtifactCategory[] = [
  "all",
  "doc",
  "sheet",
  "image",
  "av",
  "code",
  "pdf_ppt",
  "other",
];

const CAT_KEYS: Record<ArtifactCategory, MessageKey> = {
  all: "filespace.cat.all",
  doc: "filespace.cat.doc",
  sheet: "filespace.cat.sheet",
  image: "filespace.cat.image",
  av: "filespace.cat.av",
  code: "filespace.cat.code",
  pdf_ppt: "filespace.cat.pdf_ppt",
  other: "filespace.cat.other",
};

const CAT_ICONS: Record<
  ArtifactCategory,
  typeof IconWsFile
> = {
  all: IconWsLayers,
  doc: IconWsFileText,
  sheet: IconWsFileSheet,
  image: IconWsFileImage,
  av: IconWsFileVideo,
  code: IconWsFileCode,
  pdf_ppt: IconWsFileSlides,
  other: IconWsFile,
};

/** ISO 时间本地化 */
function formatDate(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString();
}

const listMainStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  display: "flex",
  alignItems: "center",
  gap: 10,
  border: "none",
  background: "transparent",
  color: "inherit",
  cursor: "pointer",
  textAlign: "left",
  padding: 0,
  font: "inherit",
};

const gridMainStyle: CSSProperties = {
  flex: 1,
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  gap: 8,
  border: "none",
  background: "transparent",
  color: "inherit",
  cursor: "pointer",
  textAlign: "center",
  padding: 0,
  font: "inherit",
  width: "100%",
};

export default function FileSpacePanel({
  active,
  onOpenSession,
  onClose,
  onAttachFiles,
  onOpenInWorkspace,
}: Props) {
  const { t } = useI18n();
  const confirm = useConfirm();
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [agentId, setAgentId] = useState("workspace");
  const [category, setCategory] = useState<ArtifactCategory>("all");
  const [recentOnly, setRecentOnly] = useState(true);
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [layout, setLayout] = useState<LayoutMode>("list");
  const [groups, setGroups] = useState<ArtifactSessionGroupDto[]>([]);
  const [counts, setCounts] = useState<Record<string, number>>({});
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [selected, setSelected] = useState<ArtifactDto | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [menuFiles, setMenuFiles] = useState<ArtifactDto[]>([]);
  const { showToast, toastHost } = useTransientToast();
  const debounceRef = useRef<number | null>(null);
  const loadGen = useRef(0);

  useEffect(() => {
    if (debounceRef.current != null) window.clearTimeout(debounceRef.current);
    debounceRef.current = window.setTimeout(() => {
      setDebouncedQuery(query.trim());
    }, 250);
    return () => {
      if (debounceRef.current != null) window.clearTimeout(debounceRef.current);
    };
  }, [query]);

  useEffect(() => {
    if (!active || !isTauri()) return;
    void (async () => {
      try {
        const cfg = await invoke<{
          memory_dir: string;
          workspace_dir: string;
          active_agent_id: string;
          agents: AgentInfo[];
        }>("get_config");
        setAgents(cfg.agents);
        setAgentId(cfg.active_agent_id);
      } catch {
        // config unavailable outside Tauri or during startup
      }
    })();
  }, [active]);

  useAgentsChanged((payload) => {
    if (!active || !isTauri()) return;
    void (async () => {
      try {
        const cfg = await invoke<{
          active_agent_id: string;
          agents: AgentInfo[];
        }>("get_config");
        setAgents(cfg.agents);
        setAgentId(cfg.active_agent_id || payload.active_agent_id);
      } catch {
        // ignore
      }
    })();
  });

  const load = useCallback(async () => {
    if (!isTauri()) {
      setGroups([]);
      setCounts({});
      return;
    }
    const gen = ++loadGen.current;
    setLoading(true);
    setError(null);
    try {
      await invoke("reconcile_artifacts");
      const result = await invoke<ListArtifactsResult>("list_artifacts", {
        category: category === "all" ? null : category,
        query: debouncedQuery || null,
        recentOnly,
        limit: 200,
        agentId,
      });
      if (gen !== loadGen.current) return;
      setGroups(result.groups ?? []);
      setCounts(result.counts ?? {});
      setExpanded((prev) => {
        const next = { ...prev };
        for (const g of result.groups ?? []) {
          const key = g.session_id ?? "__unlinked__";
          if (next[key] === undefined) next[key] = true;
        }
        return next;
      });
    } catch (e) {
      if (gen !== loadGen.current) return;
      setError(String(e));
      setGroups([]);
    } finally {
      if (gen === loadGen.current) setLoading(false);
    }
  }, [category, debouncedQuery, recentOnly, agentId]);

  const handleAgentChange = async (id: string) => {
    if (id === agentId) return;
    setAgentId(id);
    try {
      const cfg = await invoke<{
        workspace_dir: string;
        active_agent_id: string;
        agents: AgentInfo[];
      }>("set_active_agent", { agentId: id });
      setAgents(cfg.agents);
      setAgentId(cfg.active_agent_id);
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    if (!active) return;
    void load();
  }, [active, load]);

  const openSelectedExternally = async () => {
    if (!selected?.path || selected.missing) return;
    try {
      await invoke("open_path_externally", { path: selected.path });
    } catch {
      // 静默失败；预览区仍显示 unsupported
    }
  };
  const totalCount = counts.all ?? groups.reduce((n, g) => n + g.files.length, 0);

  const flatFiles = useMemo(
    () => groups.flatMap((g) => g.files.map((f) => ({ group: g, file: f }))),
    [groups],
  );

  const visibleIds = useMemo(() => {
    if (layout === "grid") return flatFiles.map(({ file }) => file.id);
    const ids: string[] = [];
    for (const g of groups) {
      const key = g.session_id ?? "__unlinked__";
      if (expanded[key] === false) continue;
      for (const f of g.files) ids.push(f.id);
    }
    return ids;
  }, [layout, flatFiles, groups, expanded]);

  const selection = useFileSelection(visibleIds);
  const selectedFiles = useMemo(() => {
    const map = new Map(flatFiles.map(({ file }) => [file.id, file]));
    return visibleIds
      .filter((id) => selection.selectedIds.has(id))
      .map((id) => map.get(id))
      .filter((f): f is ArtifactDto => !!f);
  }, [flatFiles, visibleIds, selection.selectedIds]);


  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (menu) {
        setMenu(null);
        setMenuFiles([]);
        return;
      }
      selection.clear();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [menu, selection.clear]);

  const openMenuAt = (id: string, x: number, y: number) => {
    const map = new Map(flatFiles.map(({ file }) => [file.id, file]));
    const already = selection.selectedIds.has(id);
    const files = already
      ? visibleIds
          .filter((i) => selection.selectedIds.has(i))
          .map((i) => map.get(i))
          .filter((f): f is ArtifactDto => !!f)
      : (() => {
          const f = map.get(id);
          return f ? [f] : [];
        })();
    selection.prepareMenu(id);
    setMenuFiles(files);
    setSelected(files[0] ?? null);
    setMenu({ x, y });
  };

  const runOpen = async (files: ArtifactDto[]) => {
    const existing = files.filter((f) => !f.missing);
    if (existing.length > 5) {
      showToast(t("filespace.toast.openLimit", { n: "5" }), { error: true });
    }
    for (const f of existing.slice(0, 5)) {
      try {
        await invoke("open_path_externally", { path: f.path });
      } catch {
        // ignore per-file open failures
      }
    }
  };

  const runReveal = async (files: ArtifactDto[]) => {
    if (files.length !== 1 || files[0].missing) {
      showToast(t("filespace.toast.revealSingleOnly"), { error: true });
      return;
    }
    try {
      await invoke("reveal_in_folder", { path: files[0].path });
    } catch {
      // silent
    }
  };

  const runCopyPath = async (files: ArtifactDto[]) => {
    const text = files.map((f) => f.path).join("\n");
    try {
      await navigator.clipboard.writeText(text);
      showToast(t("filespace.toast.copiedPath"), { tone: "success" });
    } catch {
      // clipboard unavailable
    }
  };

  const runCopyFile = async (files: ArtifactDto[]) => {
    const paths = files.filter((f) => !f.missing).map((f) => f.path);
    if (!paths.length) return;
    try {
      await invoke("copy_paths_to_clipboard", { paths });
      showToast(t("filespace.toast.copiedFile"), { tone: "success" });
    } catch (e) {
      showToast(String(e), { error: true });
    }
  };

  const runRemoveIndex = async (files: ArtifactDto[]) => {
    if (!files.length) return;
    try {
      await invoke("remove_artifacts_by_paths", {
        paths: files.map((f) => f.path),
      });
      showToast(t("filespace.toast.removedIndex"), { tone: "success" });
      selection.clear();
      setSelected(null);
      setMenuFiles([]);
      setMenu(null);
      await load();
    } catch (e) {
      showToast(String(e), { error: true });
    }
  };

  const runTrash = async (files: ArtifactDto[]) => {
    const existing = files.filter((f) => !f.missing);
    const missingOnly = files.filter((f) => f.missing);
    try {
      if (existing.length) {
        // Prefer per-path trash for partial success (Important from Task 2 review):
        const trashed: string[] = [];
        const fails: string[] = [];
        for (const f of existing) {
          try {
            await invoke("trash_paths", { paths: [f.path] });
            trashed.push(f.path);
          } catch (e) {
            fails.push(`${f.name}: ${String(e)}`);
          }
        }
        if (trashed.length) {
          await invoke("remove_artifacts_by_paths", { paths: trashed });
        }
        if (fails.length && trashed.length) {
          showToast(
            t("filespace.toast.partialFail", {
              ok: String(trashed.length),
              fail: String(fails.length),
              detail: fails.slice(0, 2).join("; "),
            }),
            { tone: "warning" },
          );
        } else if (fails.length && !trashed.length) {
          showToast(fails[0] ?? "trash failed", { error: true });
          return;
        } else {
          showToast(t("filespace.toast.trashed"), { tone: "success" });
        }
      }
      if (missingOnly.length) {
        await invoke("remove_artifacts_by_paths", {
          paths: missingOnly.map((f) => f.path),
        });
        if (!existing.length) {
          showToast(t("filespace.toast.removedIndex"), { tone: "success" });
        }
      }
      selection.clear();
      setSelected(null);
      setMenuFiles([]);
      setMenu(null);
      await load();
    } catch (e) {
      showToast(String(e), { error: true });
    }
  };

  const dispatchAction = async (action: FileMenuAction, files: ArtifactDto[]) => {
    switch (action) {
      case "open":
        await runOpen(files);
        break;
      case "reveal":
        await runReveal(files);
        break;
      case "copyPath":
        await runCopyPath(files);
        break;
      case "copyFile":
        await runCopyFile(files);
        break;
      case "attachNew":
        await onAttachFiles?.(
          files.filter((f) => !f.missing),
          "new",
        );
        break;
      case "attachCurrent":
        await onAttachFiles?.(
          files.filter((f) => !f.missing),
          "current",
        );
        break;
      case "trash":
        if (!files.length) break;
        if (files.every((f) => f.missing)) {
          await runRemoveIndex(files);
        } else {
          const ok = await confirm({
            title: t("filespace.confirm.trashTitle"),
            message:
              files.length === 1
                ? t("filespace.confirm.trashOne", { name: files[0].name })
                : t("filespace.confirm.trashMany", { n: String(files.length) }),
            confirmLabel: t("filespace.confirm.ok"),
            variant: "danger",
          });
          if (!ok) break;
          await runTrash(files);
        }
        break;
      case "removeIndex":
        await runRemoveIndex(files);
        break;
      case "openInWorkspace":
        if (files.length === 1 && !files[0].missing) {
          onOpenInWorkspace?.(files[0].path);
        }
        break;
    }
  };

  const handleAction = async (action: FileMenuAction) => {
    const files = menuFiles;
    setMenu(null);
    setMenuFiles([]);
    await dispatchAction(action, files);
  };

  const handleBatchAction = async (action: FileMenuAction) => {
    await dispatchAction(action, selectedFiles);
  };

  const menuItems = useMemo(() => {
    const anyExisting = menuFiles.some((f) => !f.missing);
    const allMissing =
      menuFiles.length > 0 && menuFiles.every((f) => f.missing);
    const singleOk = menuFiles.length === 1 && !menuFiles[0]?.missing;
    return [
      {
        action: "open" as const,
        labelKey: "filespace.menu.open" as const,
        disabled: !anyExisting,
      },
      {
        action: "reveal" as const,
        labelKey: "filespace.menu.reveal" as const,
        disabled: !singleOk,
      },
      ...(onOpenInWorkspace
        ? [
            {
              action: "openInWorkspace" as const,
              labelKey: "filespace.menu.openInWorkspace" as const,
              disabled: !singleOk,
            },
          ]
        : []),
      {
        action: "copyPath" as const,
        labelKey: "filespace.menu.copyPath" as const,
        disabled: menuFiles.length === 0,
        separatorBefore: true,
      },
      {
        action: "copyFile" as const,
        labelKey: "filespace.menu.copyFile" as const,
        disabled: !anyExisting,
      },
      {
        action: "attachNew" as const,
        labelKey: "filespace.menu.attachNew" as const,
        disabled: !anyExisting,
        separatorBefore: true,
      },
      {
        action: "attachCurrent" as const,
        labelKey: "filespace.menu.attachCurrent" as const,
        disabled: !anyExisting,
      },
      allMissing
        ? {
            action: "removeIndex" as const,
            labelKey: "filespace.menu.removeIndex" as const,
            label:
              menuFiles.length > 1
                ? `${t("filespace.menu.removeIndex")} (${menuFiles.length})`
                : undefined,
            danger: true,
            separatorBefore: true,
          }
        : {
            action: "trash" as const,
            labelKey: "filespace.menu.trash" as const,
            label:
              menuFiles.length > 1
                ? `${t("filespace.menu.trash")} (${menuFiles.length})`
                : undefined,
            disabled: menuFiles.length === 0,
            danger: true,
            separatorBefore: true,
          },
    ];
  }, [menuFiles, t, onOpenInWorkspace]);

  const toggleGroup = (key: string) => {
    setExpanded((prev) => ({ ...prev, [key]: !prev[key] }));
  };

  const openContinue = (sessionId: string | null, messageId?: string | null) => {
    if (!sessionId) return;
    onOpenSession(sessionId, messageId);
  };

  const canLocate =
    !!selected?.session_id && !!selected?.message_id && !selected.missing;

  return (
    <div className="filespace-panel">
      <aside className="fs-cats" aria-label={t("filespace.cat.all")}>
        <div className="fs-agent-slot">
          <AgentPicker
            agents={agents}
            value={agentId}
            onChange={(id) => void handleAgentChange(id)}
            disabled={agents.length === 0}
            labelKey="filespace.agentFilter"
            className="fs-agent-picker"
          />
        </div>
        <div className="fs-cat-list">
          {CATEGORIES.map((cat) => {
            const count = counts[cat] ?? (cat === "all" ? totalCount : 0);
            const CatIcon = CAT_ICONS[cat];
            return (
              <button
                key={cat}
                type="button"
                className={`fs-cat ${category === cat ? "is-active" : ""}`}
                onClick={() => setCategory(cat)}
              >
                <span className="fs-cat-main">
                  <span className="fs-cat-icon" aria-hidden>
                    <CatIcon width={15} height={15} />
                  </span>
                  <span className="fs-cat-label">{t(CAT_KEYS[cat])}</span>
                </span>
                <span className="fs-cat-count">{count}</span>
              </button>
            );
          })}
        </div>
      </aside>

      <div className="fs-main">
        <div className="fs-toolbar">
          <div className="fs-tabs" role="tablist">
            <button
              type="button"
              role="tab"
              aria-selected={recentOnly}
              className={`fs-tab ${recentOnly ? "is-active" : ""}`}
              onClick={() => setRecentOnly(true)}
            >
              <History size={14} strokeWidth={2.25} aria-hidden />
              {t("filespace.recent")}
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={!recentOnly}
              className={`fs-tab ${!recentOnly ? "is-active" : ""}`}
              onClick={() => setRecentOnly(false)}
            >
              <LayoutGrid size={14} strokeWidth={2.25} aria-hidden />
              {t("filespace.all")}
            </button>
          </div>

          <div className="fs-toolbar-end">
            <ExpandableSearch
              value={query}
              onChange={setQuery}
              placeholderKey="filespace.search"
            />

            <div className="fs-layout-toggle" role="group" aria-label="layout">
              <button
                type="button"
                className={`fs-layout-btn ${layout === "list" ? "is-active" : ""}`}
                onClick={() => setLayout("list")}
                title={t("filespace.list")}
                aria-label={t("filespace.list")}
                aria-pressed={layout === "list"}
              >
                <IconWsViewList width={15} height={15} />
              </button>
              <button
                type="button"
                className={`fs-layout-btn ${layout === "grid" ? "is-active" : ""}`}
                onClick={() => setLayout("grid")}
                title={t("filespace.grid")}
                aria-label={t("filespace.grid")}
                aria-pressed={layout === "grid"}
              >
                <IconWsViewGrid width={15} height={15} />
              </button>
            </div>

            {onClose && (
              <button
                type="button"
                className="fs-close-btn"
                onClick={onClose}
                title={t("filespace.back")}
                aria-label={t("filespace.back")}
              >
                <IconWsBackChat width={16} height={16} />
              </button>
            )}
          </div>
        </div>

        <div className="fs-board-stub" role="note">
          <Palette size={14} strokeWidth={2} aria-hidden />
          <span>{t("filespace.boardComingSoon")}</span>
        </div>

        <FileSpaceBatchBar
          count={selection.selectedIds.size}
          onAction={(a) => void handleBatchAction(a)}
          onClear={selection.clear}
          disableCopyFile={selectedFiles.every((f) => f.missing)}
          disableAttach={selectedFiles.every((f) => f.missing)}
          disableTrash={selectedFiles.length === 0}
        />

        <div className="fs-body">
          <AnimatedSwitch
            switchKey={recentOnly ? "recent" : "all"}
            className="anim-switch--fill"
          >
          <div className="fs-list-pane" data-selected-count={selectedFiles.length}>
            {loading && <div className="fs-status">{t("filespace.search")}…</div>}
            {error && <div className="fs-error">{error}</div>}
            {!loading && !error && groups.length === 0 && (
              <EmptyIllustration
                scene="files"
                className="fs-empty-illust"
                title={t("filespace.empty")}
              />
            )}

            {layout === "grid" ? (
              <div className="fs-grid">
                {flatFiles.map(({ file }) => (
                  <div
                    key={file.id}
                    className={`fs-grid-card ${selected?.id === file.id ? "is-selected" : ""} ${selection.selectedIds.has(file.id) ? "is-checked" : ""} ${file.missing ? "is-missing" : ""}`}
                  >
                    <input
                      type="checkbox"
                      className="fs-check"
                      checked={selection.selectedIds.has(file.id)}
                      onChange={() => selection.onCheckboxToggle(file.id)}
                      onClick={(e) => e.stopPropagation()}
                      aria-label={file.name}
                    />
                    <button
                      type="button"
                      className="fs-file-main"
                      style={gridMainStyle}
                      onClick={(e) => {
                        selection.onItemClick(file.id, e);
                        setSelected(file);
                      }}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        openMenuAt(file.id, e.clientX, e.clientY);
                      }}
                    >
                      <FileGlyph name={file.name} className="fs-file-glyph" />
                      <span className="fs-grid-name" title={file.name}>
                        {file.name}
                      </span>
                      <span className="fs-grid-meta">{formatSize(file.size)}</span>
                    </button>
                    <button
                      type="button"
                      className="fs-more-btn"
                      aria-label={t("filespace.menu.more")}
                      onClick={(e) => {
                        e.stopPropagation();
                        const r = e.currentTarget.getBoundingClientRect();
                        openMenuAt(file.id, r.left, r.bottom);
                      }}
                    >
                      <MoreHorizontal size={16} strokeWidth={2.25} aria-hidden />
                    </button>
                  </div>
                ))}
              </div>
            ) : (
              <div className="fs-groups">
                {groups.map((g) => {
                  const key = g.session_id ?? "__unlinked__";
                  const open = expanded[key] !== false;
                  const linked = !!g.session_id;
                  return (
                    <section key={key} className="fs-group">
                      <div className="fs-group-head">
                        <button
                          type="button"
                          className="fs-group-toggle"
                          onClick={() => toggleGroup(key)}
                          aria-expanded={open}
                        >
                          <span className={`fs-chevron ${open ? "is-open" : ""}`}>
                            ▸
                          </span>
                          <span className="fs-group-title">
                            {linked ? g.session_title : t("filespace.unlinked")}
                          </span>
                          <span className="fs-group-count">{g.files.length}</span>
                        </button>
                        <button
                          type="button"
                          className="fs-continue-btn"
                          disabled={!linked}
                          onClick={() => openContinue(g.session_id)}
                          title={
                            linked ? t("filespace.continue") : t("filespace.unlinked")
                          }
                        >
                          {t("filespace.continue")}
                        </button>
                      </div>
                      {open && (
                        <ul className="fs-file-list">
                          {g.files.map((file) => (
                            <li key={file.id}>
                              <div
                                className={`fs-file-row ${selected?.id === file.id ? "is-selected" : ""} ${selection.selectedIds.has(file.id) ? "is-checked" : ""} ${file.missing ? "is-missing" : ""}`}
                              >
                                <input
                                  type="checkbox"
                                  className="fs-check"
                                  checked={selection.selectedIds.has(file.id)}
                                  onChange={() => selection.onCheckboxToggle(file.id)}
                                  onClick={(e) => e.stopPropagation()}
                                  aria-label={file.name}
                                />
                                <button
                                  type="button"
                                  className="fs-file-main"
                                  style={listMainStyle}
                                  onClick={(e) => {
                                    selection.onItemClick(file.id, e);
                                    setSelected(file);
                                  }}
                                  onContextMenu={(e) => {
                                    e.preventDefault();
                                    openMenuAt(file.id, e.clientX, e.clientY);
                                  }}
                                >
                                  <FileGlyph
                                    name={file.name}
                                    className="fs-file-glyph"
                                  />
                                  <span className="fs-file-meta">
                                    <span className="fs-file-name">{file.name}</span>
                                    <span className="fs-file-sub">
                                      {formatSize(file.size)} · {formatDate(file.created_at)}
                                      {file.missing ? ` · ${t("filespace.missing")}` : ""}
                                    </span>
                                  </span>
                                </button>
                                <button
                                  type="button"
                                  className="fs-more-btn"
                                  aria-label={t("filespace.menu.more")}
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    const r = e.currentTarget.getBoundingClientRect();
                                    openMenuAt(file.id, r.left, r.bottom);
                                  }}
                                >
                                  <MoreHorizontal size={16} strokeWidth={2.25} aria-hidden />
                                </button>
                              </div>
                            </li>
                          ))}
                        </ul>
                      )}
                    </section>
                  );
                })}
              </div>
            )}
            {menu && (
              <FileContextMenu
                className="fs-glass-ctx"
                x={menu.x}
                y={menu.y}
                items={menuItems}
                onAction={(a) => void handleAction(a)}
                onClose={() => {
                  setMenu(null);
                  setMenuFiles([]);
                }}
              />
            )}
          </div>
          </AnimatedSwitch>

          <aside className="fs-preview" aria-label={t("filespace.preview")}>
            {!selected ? (
              <div className="fs-preview-empty">
                <EmptyIllustration
                  scene="files"
                  size="lg"
                  className="fs-preview-empty-illust"
                  title={t("filespace.previewEmpty")}
                  hint={t("filespace.previewEmptyHint")}
                />
              </div>
            ) : (
              <>
                <div className="fs-preview-head">
                  <FileGlyph name={selected.name} className="fs-file-glyph" />
                  <div className="fs-preview-titles">
                    <div className="fs-preview-name">{selected.name}</div>
                    <div className="fs-preview-path" title={selected.path}>
                      {selected.path}
                    </div>
                  </div>
                </div>

                <div className="fs-preview-actions">
                  <button
                    type="button"
                    className="fs-action-btn"
                    disabled={selected.missing}
                    onClick={() => void openSelectedExternally()}
                  >
                    <ExternalLink size={14} strokeWidth={2.1} aria-hidden />
                    {t("workspace.openExternally")}
                  </button>
                  <button
                    type="button"
                    className="fs-action-btn"
                    disabled={!canLocate}
                    onClick={() =>
                      openContinue(selected.session_id, selected.message_id)
                    }
                  >
                    <Navigation size={14} strokeWidth={2.1} aria-hidden />
                    {t("filespace.locate")}
                  </button>
                  <button
                    type="button"
                    className="fs-action-btn is-primary"
                    disabled={!selected.session_id}
                    onClick={() => openContinue(selected.session_id)}
                  >
                    <MessageCircle size={14} strokeWidth={2.1} aria-hidden />
                    {t("filespace.continue")}
                  </button>
                </div>

                <div className="fs-preview-body">
                  <FileSpaceViewer
                    path={selected.path}
                    name={selected.name}
                    missing={selected.missing}
                    mime={selected.mime}
                    category={selected.category}
                    onOpenExternally={() => void openSelectedExternally()}
                  />
                </div>
              </>
            )}
          </aside>
        </div>
      </div>
      {toastHost}
    </div>
  );
}
