/** Agent 工作区文件树与编辑。 */
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { Eye, FileCode2 } from "lucide-react";
import { useTheme } from "../../hooks/useTheme";
import { useTransientToast } from "../../hooks/useTransientToast";
import { useFileSelection } from "../../hooks/useFileSelection";
import { useI18n } from "../../i18n/LocaleContext";
import type { FileEntryDto } from "../../types";
import { useAgentsChanged } from "../../lib/agent/agentsChanged";
import {
  isMarkdownFilename,
  readWorkspaceMdMode,
  writeWorkspaceMdMode,
  type MdMode,
} from "../../lib/filespace/workspaceMdMode";
import { buildWorkspaceMenuItems } from "../../lib/filespace/workspaceMenuItems";
import type { AgentInfo } from "../../types/agent";
import AgentPicker from "../agents/AgentPicker";
import AnimatedSwitch from "../ui/AnimatedSwitch";
import { ChatMarkdown } from "../chat/ChatMarkdown";
import MediaPreview from "../media/MediaPreview";
import ExpandableSearch from "../ui/ExpandableSearch";
import FileContextMenu, { type FileMenuAction } from "../filespace/FileContextMenu";
import FileSpaceConfirm from "../filespace/FileSpaceConfirm";
import WorkspaceBatchBar from "./WorkspaceBatchBar";
import WorkspaceEditor from "./WorkspaceEditor";
import { EmptyIllustration } from "../../illustrations";
import {
  IconWsArrowLeft,
  IconWsArrowUp,
  IconWsBackChat,
  IconWsNewFile,
  IconWsNewFolder,
  IconWsTrash,
  IconWsViewCompact,
  IconWsViewGrid,
  IconWsViewList,
} from "./WorkspaceIcons";
import {
  isExternalOnlyFile as isExternalOnlyByType,
  mediaKindOf as mediaKindOfByType,
  resolveFileType,
  type FileGlyphKind,
} from "../../lib/filespace/fileTypeIcon";

/** 工作区面板入参 */
type Props = {
  /** 关闭面板（若由覆盖层打开） */
  onClose?: () => void;
};

/** 浏览 / 文本编辑 / 媒体预览 */
type ViewMode = "browse" | "editor" | "media";
/** 文件列表布局 */
type ListLayout = "list" | "grid" | "compact";
/** 新建条目类型 */
type CreateMode = "file" | "folder";
/** 工作区条目语义种类（决定图标与打开方式） */
type FileKind = FileGlyphKind;
/** 可内嵌预览的媒体类型 */
type MediaKind = "image" | "video" | "audio" | "html";
/** 右键菜单上下文：空白区 / 选中条目 */
type MenuKind = "blank" | "entries";

const LIST_LAYOUT_KEY = "astro-workspace-list-layout";
const MAX_OPEN_EXTERNALLY = 5;

/** 是否运行在 Tauri 壳内 */
function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** 焦点是否在可输入控件（用于忽略快捷键） */
function isTypingTarget(el: EventTarget | null): boolean {
  if (!(el instanceof HTMLElement)) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || el.isContentEditable;
}

/** 从 localStorage 读取列表布局 */
function readListLayout(): ListLayout {
  try {
    const v = localStorage.getItem(LIST_LAYOUT_KEY);
    if (v === "list" || v === "grid" || v === "compact") return v;
  } catch {
    // ignore
  }
  return "list";
}

/** 由名称与是否目录推断条目种类 */
function fileKind(name: string, isDir: boolean): FileKind {
  return resolveFileType(name, isDir).kind;
}

/** 不宜用内置文本编辑器打开的文件（用系统默认应用） */
function isExternalOnlyFile(name: string): boolean {
  return isExternalOnlyByType(name);
}

/** 若可内嵌预览则返回 image/video */
function mediaKindOf(name: string): MediaKind | null {
  return mediaKindOfByType(name);
}

/** 本地绝对路径 → Tauri 可加载的 asset URL */
function localMediaSrc(path: string): string | null {
  if (!isTauri()) return null;
  try {
    return convertFileSrc(path);
  } catch {
    return null;
  }
}

/** 人类可读文件大小 */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** 绝对路径 → `~/…` 展示 */
function formatTildePath(absPath: string): string {
  if (!absPath) return "";
  if (absPath.startsWith("~/") || absPath === "~") return absPath;
  const unixHome =
    absPath.match(/^(\/Users\/[^/]+)/)?.[1] ??
    absPath.match(/^(\/home\/[^/]+)/)?.[1] ??
    null;
  if (unixHome && absPath.startsWith(unixHome)) {
    return `~${absPath.slice(unixHome.length)}`;
  }
  const winHome = absPath.match(/^([A-Za-z]:[\\/]Users[\\/][^\\/]+)/)?.[1] ?? null;
  if (winHome && absPath.toLowerCase().startsWith(winHome.toLowerCase())) {
    return `~${absPath.slice(winHome.length).replace(/\\/g, "/")}`;
  }
  return absPath;
}

/** 从工作区根到当前目录生成面包屑 */
function buildBreadcrumbs(
  root: string,
  workspaceRoot: string,
): { label: string; path: string }[] {
  if (!root || !workspaceRoot) {
    if (root) return [{ label: formatTildePath(root), path: root }];
    return [];
  }
  const crumbs: { label: string; path: string }[] = [
    { label: formatTildePath(workspaceRoot), path: workspaceRoot },
  ];
  if (!root.startsWith(workspaceRoot) || root === workspaceRoot) {
    return crumbs;
  }
  const relative = root.slice(workspaceRoot.length).replace(/^[/\\]/, "");
  let acc = workspaceRoot;
  for (const part of relative.split(/[/\\]/).filter(Boolean)) {
    acc = `${acc}/${part}`;
    crumbs.push({ label: part, path: acc });
  }
  return crumbs;
}

/** 工作区条目图标（按后缀直接映射 Lucide） */
function FileGlyph({ name, isDir }: { name: string; isDir: boolean }) {
  const { kind, Icon } = resolveFileType(name, isDir);
  return (
    <span className="ws-file-glyph" data-kind={kind} aria-hidden>
      <Icon size={18} strokeWidth={2} />
    </span>
  );
}

export default function WorkspacePanel({ onClose }: Props) {
  const { t } = useI18n();
  const { resolved: theme } = useTheme();
  const [root, setRoot] = useState("");
  const [workspaceRoot, setWorkspaceRoot] = useState("");
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [activeAgentId, setActiveAgentId] = useState("workspace");
  const [entries, setEntries] = useState<FileEntryDto[]>([]);
  const [view, setView] = useState<ViewMode>("browse");
  const [editorPath, setEditorPath] = useState<string | null>(null);
  const [editorName, setEditorName] = useState("");
  const [savedContent, setSavedContent] = useState("");
  const [draftContent, setDraftContent] = useState("");
  const [mediaKind, setMediaKind] = useState<MediaKind | null>(null);
  const [mediaMeta, setMediaMeta] = useState<string>("");
  const [loadingList, setLoadingList] = useState(false);
  const [loadingFile, setLoadingFile] = useState(false);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [createMode, setCreateMode] = useState<CreateMode | null>(null);
  const [newName, setNewName] = useState("");
  const [creating, setCreating] = useState(false);
  const [listLayout, setListLayout] = useState<ListLayout>(() =>
    typeof window === "undefined" ? "list" : readListLayout(),
  );
  const [fileSearch, setFileSearch] = useState("");
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [menuKind, setMenuKind] = useState<MenuKind>("entries");
  const [menuPaths, setMenuPaths] = useState<string[]>([]);
  const [pendingCut, setPendingCut] = useState<string[] | null>(null);
  const [trashConfirm, setTrashConfirm] = useState<FileEntryDto[] | null>(null);
  const [renamingPath, setRenamingPath] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [mdMode, setMdMode] = useState<MdMode>(() => readWorkspaceMdMode());
  const { showToast, toastHost } = useTransientToast();
  const panelRef = useRef<HTMLElement | null>(null);
  const renameIgnoreBlurRef = useRef(false);
  const renameBusyRef = useRef(false);

  const dirty = view === "editor" && draftContent !== savedContent;
  const editorIsMarkdown = isMarkdownFilename(editorName);
  const showMdPreview = editorIsMarkdown && mdMode === "preview";

  const setMdModePersist = (mode: MdMode) => {
    setMdMode(mode);
    writeWorkspaceMdMode(mode);
  };

  const breadcrumbs = useMemo(
    () => buildBreadcrumbs(root, workspaceRoot),
    [root, workspaceRoot],
  );

  const filteredEntries = useMemo(() => {
    const q = fileSearch.trim().toLowerCase();
    if (!q) return entries;
    return entries.filter((e) => e.name.toLowerCase().includes(q));
  }, [entries, fileSearch]);

  const visibleIds = useMemo(
    () => filteredEntries.map((e) => e.path),
    [filteredEntries],
  );
  const selection = useFileSelection(visibleIds);
  const entryByPath = useMemo(
    () => new Map(entries.map((e) => [e.path, e])),
    [entries],
  );
  const selectedEntries = useMemo(
    () =>
      visibleIds
        .filter((id) => selection.selectedIds.has(id))
        .map((id) => entryByPath.get(id))
        .filter((e): e is FileEntryDto => !!e),
    [visibleIds, selection.selectedIds, entryByPath],
  );
  // Optimistic: empty/unsupported clipboard fails on paste with toast.
  const canPaste = true;

  useEffect(() => {
    try {
      localStorage.setItem(LIST_LAYOUT_KEY, listLayout);
    } catch {
      // ignore
    }
  }, [listLayout]);


  const load = useCallback(async (path?: string) => {
    setLoadingList(true);
    setError(null);
    try {
      if (!path) {
        const cfg = await invoke<{
          memory_dir: string;
          workspace_dir: string;
          active_agent_id: string;
          agents: AgentInfo[];
        }>("get_config");
        setAgents(cfg.agents);
        setActiveAgentId(cfg.active_agent_id);
        setWorkspaceRoot(cfg.workspace_dir);
        setRoot(cfg.workspace_dir);
        const list = await invoke<FileEntryDto[]>("list_files", {
          path: cfg.workspace_dir,
        });
        setEntries(list);
      } else {
        const list = await invoke<FileEntryDto[]>("list_files", { path });
        setEntries(list);
        setRoot(path);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoadingList(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useAgentsChanged(() => {
    if (dirty) {
      // 有未保存编辑时只刷新列表，不切走当前文件
      void (async () => {
        try {
          const cfg = await invoke<{
            active_agent_id: string;
            agents: AgentInfo[];
          }>("get_config");
          setAgents(cfg.agents);
        } catch {
          // ignore
        }
      })();
      return;
    }
    void load();
  });

  const switchAgent = async (agentId: string) => {
    if (agentId === activeAgentId) return;
    if (dirty && !window.confirm(t("workspace.unsavedConfirm"))) return;
    setLoadingList(true);
    setError(null);
    try {
      const cfg = await invoke<{
        workspace_dir: string;
        active_agent_id: string;
        agents: AgentInfo[];
      }>("set_active_agent", { agentId });
      setAgents(cfg.agents);
      setActiveAgentId(cfg.active_agent_id);
      setWorkspaceRoot(cfg.workspace_dir);
      setRoot(cfg.workspace_dir);
      setView("browse");
      setEditorPath(null);
      selection.clear();
      setPendingCut(null);
      setRenamingPath(null);
      const list = await invoke<FileEntryDto[]>("list_files", {
        path: cfg.workspace_dir,
      });
      setEntries(list);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoadingList(false);
    }
  };

  const openEditor = (path: string, name: string, content: string) => {
    setEditorPath(path);
    setEditorName(name);
    setSavedContent(content);
    setDraftContent(content);
    setMediaKind(null);
    setMediaMeta("");
    setView("editor");
    setError(null);
  };

  const openMedia = (
    entry: FileEntryDto,
    kind: MediaKind,
    htmlSource?: string,
  ) => {
    setEditorPath(entry.path);
    setEditorName(entry.name);
    setSavedContent(htmlSource ?? "");
    setDraftContent(htmlSource ?? "");
    setMediaKind(kind);
    setMediaMeta(formatSize(entry.size));
    setView("media");
    setError(null);
  };

  const openFolder = async (entry: FileEntryDto) => {
    setView("browse");
    setEditorPath(null);
    setMediaKind(null);
    selection.clear();
    setRenamingPath(null);
    await load(entry.path);
  };

  const openWithSystemApp = async (path: string) => {
    setError(null);
    try {
      await invoke("open_path_externally", { path });
    } catch (e) {
      setError(`${t("workspace.openExternallyFailed")}: ${String(e)}`);
    }
  };

  const openFile = async (entry: FileEntryDto) => {
    const media = mediaKindOf(entry.name);
    if (media === "html") {
      setLoadingFile(true);
      setError(null);
      try {
        const content = await invoke<string>("read_file", { path: entry.path });
        openMedia(entry, "html", content);
      } catch {
        await openWithSystemApp(entry.path);
      } finally {
        setLoadingFile(false);
      }
      return;
    }
    if (media) {
      openMedia(entry, media);
      return;
    }
    if (isExternalOnlyFile(entry.name)) {
      await openWithSystemApp(entry.path);
      return;
    }
    setLoadingFile(true);
    setError(null);
    try {
      const content = await invoke<string>("read_file", { path: entry.path });
      openEditor(entry.path, entry.name, content);
    } catch {
      // 二进制 / 非 UTF-8：用系统默认应用打开，不展示底层错误
      await openWithSystemApp(entry.path);
    } finally {
      setLoadingFile(false);
    }
  };

  const openEntry = (entry: FileEntryDto) => {
    if (entry.is_dir) {
      void openFolder(entry);
      return;
    }
    void openFile(entry);
  };

  const goUp = async () => {
    if (!workspaceRoot || root === workspaceRoot) return;
    const parent = root.replace(/\/$/, "").split("/").slice(0, -1).join("/") || workspaceRoot;
    setView("browse");
    setEditorPath(null);
    setMediaKind(null);
    selection.clear();
    setRenamingPath(null);
    await load(parent);
  };

  const backToBrowse = () => {
    if (view === "editor" && dirty && !window.confirm(t("workspace.unsavedConfirm"))) {
      return;
    }
    setView("browse");
    setEditorPath(null);
    setMediaKind(null);
    setError(null);
  };

  const openCurrentExternally = async () => {
    if (!editorPath) return;
    await openWithSystemApp(editorPath);
  };

  const undoEdits = () => {
    setDraftContent(savedContent);
  };

  const saveFile = async () => {
    if (!editorPath) return;
    setSaving(true);
    setError(null);
    try {
      await invoke("write_file", { path: editorPath, content: draftContent });
      setSavedContent(draftContent);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const clearViewer = () => {
    setView("browse");
    setEditorPath(null);
    setMediaKind(null);
    setMediaMeta("");
    setError(null);
  };

  const resolvePaths = (paths: string[]): FileEntryDto[] =>
    paths.map((p) => entryByPath.get(p)).filter((e): e is FileEntryDto => !!e);

  const openMenuAt = (path: string, x: number, y: number) => {
    const already = selection.selectedIds.has(path);
    const paths = already
      ? visibleIds.filter((id) => selection.selectedIds.has(id))
      : [path];
    selection.prepareMenu(path);
    setMenuPaths(paths);
    setMenuKind("entries");
    setMenu({ x, y });
  };

  const openBlankMenu = (x: number, y: number) => {
    setMenuPaths([]);
    setMenuKind("blank");
    setMenu({ x, y });
  };

  const closeMenu = useCallback(() => {
    setMenu(null);
    setMenuPaths([]);
  }, []);

  const beginRename = useCallback((entry: FileEntryDto) => {
    renameIgnoreBlurRef.current = false;
    renameBusyRef.current = false;
    setRenamingPath(entry.path);
    setRenameDraft(entry.name);
  }, []);

  const cancelRename = useCallback(() => {
    setRenamingPath(null);
    setRenameDraft("");
  }, []);

  const submitRename = useCallback(async () => {
    if (!renamingPath || renameBusyRef.current) return;
    const name = renameDraft.trim();
    if (!name) {
      cancelRename();
      return;
    }
    const prev = entryByPath.get(renamingPath);
    if (prev && prev.name === name) {
      cancelRename();
      return;
    }
    if (
      dirty &&
      editorPath === renamingPath &&
      !window.confirm(t("workspace.renameUnsavedConfirm"))
    ) {
      cancelRename();
      return;
    }
    renameBusyRef.current = true;
    try {
      const dto = await invoke<FileEntryDto>("rename_path", {
        path: renamingPath,
        newName: name,
      });
      if (editorPath === renamingPath) {
        setEditorPath(dto.path);
        setEditorName(dto.name);
      }
      cancelRename();
      showToast(t("workspace.toast.renamed"), { tone: "success" });
      await load(root);
      selection.setOnly(dto.path);
    } catch (e) {
      showToast(String(e), { error: true });
    } finally {
      renameBusyRef.current = false;
    }
  }, [
    renamingPath,
    renameDraft,
    entryByPath,
    dirty,
    editorPath,
    t,
    cancelRename,
    load,
    root,
    selection,
  ]);

  const runOpen = async (targets: FileEntryDto[]) => {
    if (!targets.length) return;
    const files = targets.filter((e) => !e.is_dir);
    if (files.length) {
      if (files.length > MAX_OPEN_EXTERNALLY) {
        showToast(t("workspace.toast.openLimit", { n: String(MAX_OPEN_EXTERNALLY) }), { error: true });
      }
      for (const entry of files.slice(0, MAX_OPEN_EXTERNALLY)) {
        openEntry(entry);
      }
      return;
    }
    // Only folders selected: enter the first one (skip the rest).
    openEntry(targets[0]);
  };

  const clipboardFailToast = useCallback(
    (err: unknown) => {
      showToast(`${String(err)} · ${t("workspace.toast.clipboardHint")}`, { error: true });
    },
    [t, showToast],
  );

  const runCut = useCallback(
    async (targets: FileEntryDto[]) => {
      const paths = targets.map((e) => e.path);
      if (!paths.length) return;
      try {
        await invoke("copy_paths_to_clipboard", { paths });
        setPendingCut(paths);
        showToast(t("workspace.toast.cut"), { tone: "info" });
      } catch (e) {
        clipboardFailToast(e);
      }
    },
    [t, clipboardFailToast, showToast],
  );

  const runCopyFile = useCallback(
    async (targets: FileEntryDto[]) => {
      const paths = targets.map((e) => e.path);
      if (!paths.length) return;
      try {
        await invoke("copy_paths_to_clipboard", { paths });
        setPendingCut(null);
        showToast(t("workspace.toast.copied"), { tone: "success" });
      } catch (e) {
        clipboardFailToast(e);
      }
    },
    [t, clipboardFailToast, showToast],
  );

  const runPaste = useCallback(async () => {
    if (!root) return;
    try {
      let count = 0;
      if (pendingCut?.length) {
        const moved = await invoke<FileEntryDto[]>("move_paths", {
          sources: pendingCut,
          destDir: root,
        });
        count = moved.length;
        setPendingCut(null);
      } else {
        const pasted = await invoke<FileEntryDto[]>("paste_paths_from_clipboard", {
          destDir: root,
          mode: null,
        });
        count = pasted.length;
      }
      showToast(t("workspace.toast.pasted", { n: String(count) }), {
        tone: "success",
      });
      await load(root);
    } catch (e) {
      clipboardFailToast(e);
    }
  }, [root, pendingCut, t, load, clipboardFailToast]);

  const runCopyPath = async (targets: FileEntryDto[]) => {
    const text = targets.map((e) => e.path).join("\n");
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      showToast(t("workspace.toast.copiedPath"), { tone: "success" });
    } catch {
      // clipboard unavailable
    }
  };

  const runReveal = async (targets: FileEntryDto[]) => {
    if (targets.length !== 1) return;
    try {
      await invoke("reveal_in_folder", { path: targets[0].path });
    } catch (e) {
      showToast(String(e), { error: true });
    }
  };

  const runOpenExternally = async (targets: FileEntryDto[]) => {
    if (!targets.length) return;
    if (targets.length > MAX_OPEN_EXTERNALLY) {
      showToast(t("workspace.toast.openLimit", { n: String(MAX_OPEN_EXTERNALLY) }), { error: true });
    }
    for (const entry of targets.slice(0, MAX_OPEN_EXTERNALLY)) {
      try {
        await invoke("open_path_externally", { path: entry.path });
      } catch {
        // ignore per-file failures
      }
    }
  };

  const runTrash = async (targets: FileEntryDto[]) => {
    if (!targets.length) return;
    const trashed: string[] = [];
    const fails: string[] = [];
    setDeleting(true);
    try {
      for (const entry of targets) {
        try {
          await invoke("trash_paths", { paths: [entry.path] });
          trashed.push(entry.path);
        } catch (e) {
          fails.push(`${entry.name}: ${String(e)}`);
        }
      }
      if (fails.length && trashed.length) {
        showToast(
          t("workspace.toast.partialFail", {
            ok: String(trashed.length),
            fail: String(fails.length),
            detail: fails.slice(0, 2).join("; "),
          }),
          { tone: "warning" },
        );
      } else if (fails.length && !trashed.length) {
        showToast(fails[0] ?? "trash failed", { error: true });
        setTrashConfirm(null);
        return;
      } else {
        showToast(t("workspace.toast.trashed"), { tone: "success" });
      }
      if (editorPath && trashed.includes(editorPath)) {
        clearViewer();
      }
      if (pendingCut) {
        const left = pendingCut.filter((p) => !trashed.includes(p));
        setPendingCut(left.length ? left : null);
      }
      if (renamingPath && trashed.includes(renamingPath)) {
        cancelRename();
      }
      selection.clear();
      setTrashConfirm(null);
      closeMenu();
      await load(root);
    } catch (e) {
      showToast(String(e), { error: true });
      setTrashConfirm(null);
    } finally {
      setDeleting(false);
    }
  };

  const requestTrash = useCallback(
    (targets: FileEntryDto[]) => {
      if (!targets.length) return;
      if (
        dirty &&
        editorPath &&
        targets.some((e) => e.path === editorPath) &&
        !window.confirm(t("workspace.deleteUnsavedConfirm"))
      ) {
        return;
      }
      setTrashConfirm(targets);
    },
    [dirty, editorPath, t],
  );

  const dispatchAction = async (action: FileMenuAction, targets: FileEntryDto[]) => {
    switch (action) {
      case "open":
        await runOpen(targets);
        break;
      case "newFile":
        setCreateMode("file");
        setNewName("");
        break;
      case "newFolder":
        setCreateMode("folder");
        setNewName("");
        break;
      case "cut":
        await runCut(targets);
        break;
      case "copyFile":
        await runCopyFile(targets);
        break;
      case "paste":
        await runPaste();
        break;
      case "copyPath":
        await runCopyPath(targets);
        break;
      case "rename":
        if (targets.length === 1) beginRename(targets[0]);
        break;
      case "reveal":
        await runReveal(targets);
        break;
      case "openExternally":
        await runOpenExternally(targets);
        break;
      case "trash":
        requestTrash(targets);
        break;
      default:
        break;
    }
  };

  const handleMenuAction = async (action: FileMenuAction) => {
    const targets =
      menuKind === "blank" ? [] : resolvePaths(menuPaths.length ? menuPaths : [...selection.selectedIds]);
    closeMenu();
    await dispatchAction(action, targets);
  };

  const handleBatchAction = async (action: FileMenuAction) => {
    await dispatchAction(action, selectedEntries);
  };

  const onRowClick = (entry: FileEntryDto, e: ReactMouseEvent) => {
    if (renamingPath === entry.path) return;
    selection.onItemClick(entry.path, e);
    if (e.metaKey || e.ctrlKey || e.shiftKey) return;
    openEntry(entry);
  };

  const deleteCurrentFile = async () => {
    if (!editorPath) return;
    requestTrash([
      {
        path: editorPath,
        name: editorName,
        is_dir: false,
        size: 0,
      },
    ]);
  };

  const submitCreate = async (e: FormEvent) => {
    e.preventDefault();
    const name = newName.trim();
    if (!name || !root || !createMode) return;
    setCreating(true);
    setError(null);
    try {
      const entry = await invoke<FileEntryDto>(
        createMode === "file" ? "create_file" : "create_directory",
        { parent: root, name },
      );
      setCreateMode(null);
      setNewName("");
      await load(root);
      if (createMode === "file") {
        openEditor(entry.path, entry.name, "");
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setCreating(false);
    }
  };

  const cancelCreate = () => {
    setCreateMode(null);
    setNewName("");
  };

  const menuItems = useMemo(
    () =>
      buildWorkspaceMenuItems({
        kind: menuKind,
        selectedCount: menuKind === "blank" ? 0 : menuPaths.length,
        canPaste,
      }),
    [menuKind, menuPaths.length, canPaste],
  );

  useEffect(() => {
    const panelFocused = () => {
      const el = document.activeElement;
      return !!panelRef.current && (el === panelRef.current || panelRef.current.contains(el));
    };

    const onKey = (e: KeyboardEvent) => {
      if (view !== "browse") return;
      if (isTypingTarget(e.target)) {
        if (e.key === "Escape" && renamingPath) {
          e.preventDefault();
          cancelRename();
        }
        return;
      }
      if (!panelFocused()) return;

      const mod = e.metaKey || e.ctrlKey;
      const targets = selectedEntries;

      if (e.key === "Escape") {
        e.preventDefault();
        if (menu) {
          closeMenu();
          return;
        }
        if (renamingPath) {
          cancelRename();
          return;
        }
        selection.clear();
        return;
      }

      if (e.key === "F2") {
        e.preventDefault();
        if (targets.length === 1) beginRename(targets[0]);
        return;
      }

      if (e.key === "Delete" || e.key === "Backspace") {
        if (!targets.length) return;
        e.preventDefault();
        requestTrash(targets);
        return;
      }

      if (!mod) return;

      const key = e.key.toLowerCase();
      if (key === "a") {
        e.preventDefault();
        if (!visibleIds.length) return;
        selection.onItemClick(visibleIds[0], {});
        if (visibleIds.length > 1) {
          selection.onItemClick(visibleIds[visibleIds.length - 1], { shiftKey: true });
        }
        return;
      }
      if (key === "c") {
        if (!targets.length) return;
        e.preventDefault();
        void runCopyFile(targets);
        return;
      }
      if (key === "x") {
        if (!targets.length) return;
        e.preventDefault();
        void runCut(targets);
        return;
      }
      if (key === "v") {
        e.preventDefault();
        void runPaste();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [
    view,
    renamingPath,
    menu,
    selectedEntries,
    visibleIds,
    selection.clear,
    selection.onItemClick,
    cancelRename,
    closeMenu,
    beginRename,
    requestTrash,
    runCopyFile,
    runCut,
    runPaste,
  ]);

  return (
    <aside
      className="workspace-panel"
      ref={panelRef}
      tabIndex={0}
      onMouseDown={(e) => {
        if (isTypingTarget(e.target)) return;
        if (panelRef.current && !panelRef.current.contains(document.activeElement)) {
          panelRef.current.focus({ preventScroll: true });
        }
      }}
    >
      <AnimatedSwitch switchKey={view} className="anim-switch--fill">
      {view === "browse" ? (
        <>
          <div className="ws-toolbar">
            <h2 className="ws-toolbar-title">{t("workspace.title")}</h2>
            <div className="ws-toolbar-actions">
              <button
                type="button"
                className="ws-tool-btn"
                onClick={() => {
                  setCreateMode("file");
                  setNewName("");
                }}
                title={t("workspace.newFile")}
                aria-label={t("workspace.newFile")}
              >
                <IconWsNewFile width={18} height={18} />
              </button>
              <button
                type="button"
                className="ws-tool-btn"
                onClick={() => {
                  setCreateMode("folder");
                  setNewName("");
                }}
                title={t("workspace.newFolder")}
                aria-label={t("workspace.newFolder")}
              >
                <IconWsNewFolder width={18} height={18} />
              </button>
              {onClose && (
                <button
                  type="button"
                  className="ws-tool-btn"
                  onClick={onClose}
                  title={t("workspace.back")}
                  aria-label={t("workspace.back")}
                >
                  <IconWsBackChat width={18} height={18} />
                </button>
              )}
            </div>
          </div>

          {createMode && (
            <form className="ws-create-row" onSubmit={(e) => void submitCreate(e)}>
              <input
                className="ws-create-input"
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                placeholder={
                  createMode === "file"
                    ? t("workspace.newFilePlaceholder")
                    : t("workspace.newFolderPlaceholder")
                }
                autoFocus
                disabled={creating}
              />
              <button
                type="submit"
                className="ghost-btn active"
                disabled={creating || !newName.trim()}
                data-tip={t("workspace.create")}
              >
                {creating ? "…" : t("workspace.create")}
              </button>
              <button
                type="button"
                className="ghost-btn"
                onClick={cancelCreate}
                disabled={creating}
                data-tip={t("workspace.cancel")}
              >
                {t("workspace.cancel")}
              </button>
            </form>
          )}

          <div className="ws-path-row">
            {agents.length > 0 && (
              <AgentPicker
                agents={agents}
                value={activeAgentId}
                onChange={(id) => void switchAgent(id)}
                disabled={loadingList}
                labelKey="workspace.switchAgent"
                className="ws-agent-picker"
              />
            )}
            <button
              type="button"
              className="ws-icon-btn"
              onClick={() => void goUp()}
              title={t("workspace.up")}
              aria-label={t("workspace.up")}
              disabled={!workspaceRoot || root === workspaceRoot}
            >
              <IconWsArrowUp width={16} height={16} />
            </button>
            <nav className="ws-breadcrumb" aria-label={t("workspace.title")}>
              {breadcrumbs.map((crumb, i) => (
                <span key={crumb.path} className="ws-crumb-wrap">
                  {i > 0 && <span className="ws-crumb-sep">/</span>}
                  <button
                    type="button"
                    className={`ws-crumb ${i === breadcrumbs.length - 1 ? "is-current" : ""}`}
                    onClick={() => {
                      selection.clear();
                      void load(crumb.path);
                    }}
                    disabled={i === breadcrumbs.length - 1}
                  >
                    {crumb.label}
                  </button>
                </span>
              ))}
            </nav>
            <div className="ws-path-actions">
              <ExpandableSearch
                value={fileSearch}
                onChange={setFileSearch}
                placeholderKey="workspace.search"
              />
              <div className="ws-layout-toggle" role="group" aria-label={t("workspace.viewMode")}>
                {(
                  [
                    { id: "list" as const, Icon: IconWsViewList, labelKey: "workspace.viewList" as const },
                    { id: "grid" as const, Icon: IconWsViewGrid, labelKey: "workspace.viewGrid" as const },
                    { id: "compact" as const, Icon: IconWsViewCompact, labelKey: "workspace.viewCompact" as const },
                  ] as const
                ).map(({ id, Icon, labelKey }) => (
                  <button
                    key={id}
                    type="button"
                    className={`ws-layout-btn ${listLayout === id ? "is-active" : ""}`}
                    onClick={() => setListLayout(id)}
                    title={t(labelKey)}
                    aria-label={t(labelKey)}
                    aria-pressed={listLayout === id}
                  >
                    <Icon width={15} height={15} />
                  </button>
                ))}
              </div>
            </div>
          </div>

          <WorkspaceBatchBar
            count={selectedEntries.length}
            onAction={(a) => void handleBatchAction(a)}
            onClear={selection.clear}
          />

          {error && <div className="side-error">{error}</div>}

          <div
            className={`ws-file-tree is-${listLayout}`}
            onClick={(e) => {
              if ((e.target as HTMLElement).closest(".ws-file-row-wrap")) return;
              selection.clear();
            }}
            onContextMenu={(e) => {
              const target = e.target as HTMLElement;
              if (target.closest(".ws-file-row-wrap")) return;
              e.preventDefault();
              openBlankMenu(e.clientX, e.clientY);
            }}
          >
            {loadingList ? (
              <p className="ws-muted">{t("workspace.loading")}</p>
            ) : entries.length === 0 ? (
              <EmptyIllustration
                scene="workspace"
                className="ws-empty-illust"
                title={t("workspace.empty")}
              />
            ) : filteredEntries.length === 0 ? (
              <EmptyIllustration
                scene="workspace"
                size="sm"
                className="ws-empty-illust"
                title={t("workspace.searchEmpty")}
              />
            ) : (
              filteredEntries.map((entry) => {
                const kind = fileKind(entry.name, entry.is_dir);
                const meta = entry.is_dir ? t("workspace.folder") : formatSize(entry.size);
                const thumb =
                  listLayout === "grid" && kind === "image"
                    ? localMediaSrc(entry.path)
                    : null;
                const checked = selection.selectedIds.has(entry.path);
                const isRenaming = renamingPath === entry.path;
                return (
                  <div
                    key={entry.path}
                    className={`ws-file-row-wrap ${checked ? "is-checked" : ""} ${
                      pendingCut?.includes(entry.path) ? "is-cut" : ""
                    }`}
                    onClick={(e) => e.stopPropagation()}
                    onContextMenu={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      openMenuAt(entry.path, e.clientX, e.clientY);
                    }}
                  >
                    {isRenaming ? (
                      <div className={`ws-file-row ${checked ? "is-checked" : ""}`} data-kind={kind}>
                        {thumb ? (
                          <span className="ws-file-thumb" aria-hidden>
                            <img src={thumb} alt="" loading="lazy" />
                          </span>
                        ) : (
                          <FileGlyph name={entry.name} isDir={entry.is_dir} />
                        )}
                        <input
                          className="ws-rename-input"
                          value={renameDraft}
                          autoFocus
                          onClick={(e) => e.stopPropagation()}
                          onChange={(e) => setRenameDraft(e.target.value)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") {
                              e.preventDefault();
                              e.stopPropagation();
                              renameIgnoreBlurRef.current = true;
                              void submitRename();
                            } else if (e.key === "Escape") {
                              e.preventDefault();
                              e.stopPropagation();
                              renameIgnoreBlurRef.current = true;
                              cancelRename();
                            }
                          }}
                          onBlur={() => {
                            if (renameIgnoreBlurRef.current) {
                              renameIgnoreBlurRef.current = false;
                              return;
                            }
                            void submitRename();
                          }}
                        />
                        <span className="ws-file-meta">{meta}</span>
                      </div>
                    ) : (
                      <button
                        type="button"
                        className={`ws-file-row ${checked ? "is-checked" : ""}`}
                        data-kind={kind}
                        onClick={(e) => onRowClick(entry, e)}
                        disabled={loadingFile}
                      >
                        {thumb ? (
                          <span className="ws-file-thumb" aria-hidden>
                            <img src={thumb} alt="" loading="lazy" />
                          </span>
                        ) : (
                          <FileGlyph name={entry.name} isDir={entry.is_dir} />
                        )}
                        <span className="ws-file-name">{entry.name}</span>
                        <span className="ws-file-meta">{meta}</span>
                      </button>
                    )}
                    <button
                      type="button"
                      className="ws-file-delete"
                      onClick={(e) => {
                        e.stopPropagation();
                        requestTrash([entry]);
                      }}
                      data-tip={t("workspace.menu.trash")}
                      data-tip-pos="left"
                      aria-label={`${t("workspace.menu.trash")} ${entry.name}`}
                    >
                      <IconWsTrash width={16} height={16} />
                    </button>
                  </div>
                );
              })
            )}
          </div>

          {menu && (
            <FileContextMenu
              className="ws-ctx-menu"
              x={menu.x}
              y={menu.y}
              items={menuItems}
              onAction={(a) => void handleMenuAction(a)}
              onClose={closeMenu}
            />
          )}
          {trashConfirm && (
            <FileSpaceConfirm
              title={t("workspace.confirm.trashTitle")}
              message={
                trashConfirm.length === 1
                  ? t("workspace.confirm.trashOne", { name: trashConfirm[0].name })
                  : t("workspace.confirm.trashMany", {
                      n: String(trashConfirm.length),
                    })
              }
              confirmLabel={t("workspace.confirm.ok")}
              onCancel={() => setTrashConfirm(null)}
              onConfirm={() => void runTrash(trashConfirm)}
            />
          )}
        </>
      ) : view === "media" ? (
        <>
          <div className="ws-editor-toolbar">
            <button
              type="button"
              className="ws-tool-btn ws-tool-btn--text"
              onClick={backToBrowse}
              title={t("workspace.backToList")}
              aria-label={t("workspace.backToList")}
            >
              <IconWsArrowLeft width={18} height={18} />
            </button>
            <div className="ws-editor-actions">
              {mediaKind === "html" && editorPath ? (
                <button
                  type="button"
                  className="ghost-btn"
                  onClick={() =>
                    openEditor(editorPath, editorName, draftContent)
                  }
                >
                  {t("media.htmlSource")}
                </button>
              ) : null}
              <button
                type="button"
                className="ghost-btn"
                onClick={() => void openCurrentExternally()}
                disabled={!editorPath}
                data-tip={t("workspace.openExternally")}
              >
                {t("workspace.openExternally")}
              </button>
              <button
                type="button"
                className="ws-tool-btn ws-delete-btn"
                onClick={() => void deleteCurrentFile()}
                disabled={deleting}
                title={t("workspace.delete")}
                aria-label={t("workspace.delete")}
              >
                <IconWsTrash width={18} height={18} />
              </button>
            </div>
          </div>

          <div className="ws-editor-head">
            <FileGlyph name={editorName} isDir={false} />
            <div className="ws-editor-meta-block">
              <h3 className="ws-editor-filename">{editorName}</h3>
              <p className="ws-editor-path">
                {mediaKind === "video"
                  ? t("workspace.mediaVideo")
                  : mediaKind === "audio"
                    ? t("workspace.mediaAudio")
                    : mediaKind === "html"
                      ? t("workspace.mediaHtml")
                      : t("workspace.mediaImage")}
                {mediaMeta ? ` · ${mediaMeta}` : ""}
              </p>
            </div>
          </div>

          {error && <div className="side-error">{error}</div>}

          <div className="ws-media-stage" data-kind={mediaKind ?? "image"}>
            <div className="ws-media-frame">
              {editorPath && mediaKind ? (
                <MediaPreview
                  kind={mediaKind}
                  path={editorPath}
                  htmlSource={mediaKind === "html" ? draftContent : null}
                  alt={editorName}
                />
              ) : (
                <div className="ws-media-fallback">
                  <FileGlyph name={editorName} isDir={false} />
                  <p>{t("workspace.mediaLoadError")}</p>
                  <button
                    type="button"
                    className="ghost-btn active"
                    onClick={() => void openCurrentExternally()}
                  >
                    {t("workspace.openExternally")}
                  </button>
                </div>
              )}
            </div>
          </div>
          {trashConfirm && (
            <FileSpaceConfirm
              title={t("workspace.confirm.trashTitle")}
              message={
                trashConfirm.length === 1
                  ? t("workspace.confirm.trashOne", { name: trashConfirm[0].name })
                  : t("workspace.confirm.trashMany", {
                      n: String(trashConfirm.length),
                    })
              }
              confirmLabel={t("workspace.confirm.ok")}
              onCancel={() => setTrashConfirm(null)}
              onConfirm={() => void runTrash(trashConfirm)}
            />
          )}
        </>
      ) : (
        <>
          <div className="ws-editor-toolbar">
            <button
              type="button"
              className="ws-tool-btn ws-tool-btn--text"
              onClick={backToBrowse}
              title={t("workspace.backToList")}
              aria-label={t("workspace.backToList")}
            >
              <IconWsArrowLeft width={18} height={18} />
            </button>
            <div className="ws-editor-actions">
              {dirty && (
                <>
                  <button
                    type="button"
                    className="ghost-btn"
                    onClick={undoEdits}
                    disabled={saving || deleting}
                    data-tip={t("workspace.undo")}
                  >
                    {t("workspace.undo")}
                  </button>
                  <button
                    type="button"
                    className="ghost-btn active"
                    onClick={() => void saveFile()}
                    disabled={saving || deleting}
                    data-tip={t("workspace.save")}
                  >
                    {saving ? "…" : t("workspace.save")}
                  </button>
                </>
              )}
              <button
                type="button"
                className="ws-tool-btn ws-delete-btn"
                onClick={() => void deleteCurrentFile()}
                disabled={saving || deleting}
                title={t("workspace.delete")}
                aria-label={t("workspace.delete")}
              >
                <IconWsTrash width={16} height={16} />
              </button>
            </div>
          </div>

          <div className="ws-editor-head">
            <FileGlyph name={editorName} isDir={false} />
            <div className="ws-editor-meta-block">
              <h3 className="ws-editor-filename">{editorName}</h3>
              <p className="ws-editor-path">{editorPath}</p>
            </div>
            {editorIsMarkdown && (
              <div
                className="ws-md-modes"
                role="tablist"
                aria-label={t("workspace.previewMode")}
              >
                <button
                  type="button"
                  role="tab"
                  aria-selected={mdMode === "preview"}
                  className={`ws-md-mode ${mdMode === "preview" ? "is-active" : ""}`}
                  onClick={() => setMdModePersist("preview")}
                >
                  <Eye size={13} strokeWidth={2.3} aria-hidden />
                  {t("workspace.previewMode")}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={mdMode === "source"}
                  className={`ws-md-mode ${mdMode === "source" ? "is-active" : ""}`}
                  onClick={() => setMdModePersist("source")}
                >
                  <FileCode2 size={13} strokeWidth={2.3} aria-hidden />
                  {t("workspace.previewSource")}
                </button>
              </div>
            )}
            {dirty && <span className="ws-dirty-badge">{t("workspace.unsaved")}</span>}
          </div>

          {error && <div className="side-error">{error}</div>}

          <div className="ws-editor-wrap">
            {showMdPreview ? (
              <div className="ws-md-preview">
                <ChatMarkdown content={draftContent} />
              </div>
            ) : (
              <WorkspaceEditor
                value={draftContent}
                filename={editorName}
                theme={theme}
                onChange={setDraftContent}
              />
            )}
          </div>
          {trashConfirm && (
            <FileSpaceConfirm
              title={t("workspace.confirm.trashTitle")}
              message={
                trashConfirm.length === 1
                  ? t("workspace.confirm.trashOne", { name: trashConfirm[0].name })
                  : t("workspace.confirm.trashMany", {
                      n: String(trashConfirm.length),
                    })
              }
              confirmLabel={t("workspace.confirm.ok")}
              onCancel={() => setTrashConfirm(null)}
              onConfirm={() => void runTrash(trashConfirm)}
            />
          )}
        </>
      )}
      </AnimatedSwitch>
      {toastHost}
    </aside>
  );
}
