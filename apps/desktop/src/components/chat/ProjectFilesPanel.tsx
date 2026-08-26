import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ComponentProps,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Folder,
  FolderOpen,
  PanelRightClose,
  RefreshCw,
  Search,
} from "lucide-react";
import {
  ChevronDown as ChevronDownData,
  ChevronRight as ChevronRightData,
} from "lucide";
import type { FileEntryDto } from "../../types";
import type { ProjectFileWorkbench } from "../../hooks/chat/useProjectFileWorkbench";
import { useAppDialog } from "../../hooks/ui/DialogContext";
import { useTransientToast } from "../../hooks/ui/useTransientToast";
import { useI18n } from "../../i18n/LocaleContext";
import FileContextMenu, { type FileMenuAction } from "../filespace/FileContextMenu";
import FileTypeIcon from "../filespace/FileTypeIcon";
import { MorphToggleIcon } from "../icons/MorphIcon";

const WIDTH_KEY = "astro.projectFiles.width";
const DEFAULT_WIDTH = 264;
const MIN_WIDTH = 220;
const MAX_WIDTH = 440;

function initialWidth(): number {
  try {
    const value = Number(localStorage.getItem(WIDTH_KEY));
    return Number.isFinite(value) ? Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, value)) : DEFAULT_WIDTH;
  } catch {
    return DEFAULT_WIDTH;
  }
}

type TreeRowProps = {
  entry: FileEntryDto;
  level: number;
  workbench: ProjectFileWorkbench;
  query: string;
  onContextMenu: (entry: FileEntryDto, x: number, y: number) => void;
};

function TreeRow({ entry, level, workbench, query, onContextMenu }: TreeRowProps) {
  const expanded = workbench.expandedDirectories.has(entry.path);
  const loading = workbench.loadingDirectories.has(entry.path);
  const children = workbench.entriesByDirectory[entry.path] ?? [];
  const visibleChildren = query
    ? children.filter((child) => child.name.toLocaleLowerCase().includes(query))
    : children;

  return (
    <div role="treeitem" aria-expanded={entry.is_dir ? expanded : undefined}>
      <button
        type="button"
        className={`project-file-row${workbench.activeTab?.path === entry.path ? " is-active" : ""}`}
        style={{ "--tree-level": level } as CSSProperties}
        onClick={() => {
          if (entry.is_dir) workbench.toggleDirectory(entry.path);
          else workbench.openFile(entry);
        }}
        onContextMenu={(event) => {
          event.preventDefault();
          event.stopPropagation();
          onContextMenu(entry, event.clientX, event.clientY);
        }}
        onKeyDown={(event) => {
          if (event.key !== "ContextMenu" && !(event.shiftKey && event.key === "F10")) return;
          event.preventDefault();
          const rect = event.currentTarget.getBoundingClientRect();
          onContextMenu(entry, rect.left + 24, rect.top + rect.height / 2);
        }}
        title={entry.path}
      >
        {entry.is_dir ? (
          <MorphToggleIcon
            className="project-file-chevron"
            active={expanded}
            activeIcon={ChevronDownData}
            inactiveIcon={ChevronRightData}
            size={14}
            aria-hidden
          />
        ) : (
          <span className="project-file-chevron-spacer" />
        )}
        <FileTypeIcon
          className="project-file-icon"
          name={entry.name}
          isDir={entry.is_dir}
          expanded={expanded}
          size={16}
        />
        <span className="project-file-name">{entry.name}</span>
        {loading ? <span className="project-file-loading" aria-label="加载中" /> : null}
      </button>
      {entry.is_dir && expanded ? (
        <div role="group">
          {visibleChildren.map((child) => (
            <TreeRow
              key={child.path}
              entry={child}
              level={level + 1}
              workbench={workbench}
              query={query}
              onContextMenu={onContextMenu}
            />
          ))}
          {!loading && children.length === 0 ? (
            <div className="project-file-empty-row" style={{ "--tree-level": level + 1 } as CSSProperties}>
              空目录
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

export default function ProjectFilesPanel({
  workbench,
  onWidthChange,
}: {
  workbench: ProjectFileWorkbench;
  onWidthChange?: (width: number) => void;
}) {
  const [width, setWidth] = useState(initialWidth);
  const [query, setQuery] = useState("");
  const [menu, setMenu] = useState<{ x: number; y: number; entry: FileEntryDto } | null>(null);
  const dragRef = useRef<{ id: number; x: number; width: number } | null>(null);
  const { t } = useI18n();
  const dialog = useAppDialog();
  const { showToast, toastHost } = useTransientToast();

  useEffect(() => {
    onWidthChange?.(width);
  }, [onWidthChange, width]);

  const updateWidth = useCallback((value: number, persist = false) => {
    const next = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, Math.round(value)));
    setWidth(next);
    if (persist) {
      try {
        localStorage.setItem(WIDTH_KEY, String(next));
      } catch {
        // ignore
      }
    }
  }, []);

  const onPointerMove = (event: PointerEvent<HTMLButtonElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.id !== event.pointerId) return;
    updateWidth(drag.width + drag.x - event.clientX);
  };

  const finishResize = (event: PointerEvent<HTMLButtonElement>) => {
    if (dragRef.current?.id !== event.pointerId) return;
    dragRef.current = null;
    updateWidth(width, true);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const onResizeKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    updateWidth(width + (event.key === "ArrowLeft" ? 16 : -16), true);
  };

  const normalizedQuery = query.trim().toLocaleLowerCase();
  const rootPaths = new Set(workbench.roots.map((root) => root.path));

  const openContextMenu = (entry: FileEntryDto, x: number, y: number) => {
    setMenu({ x, y, entry });
  };

  const runMenuAction = async (action: FileMenuAction) => {
    if (!menu) return;
    const { entry } = menu;
    setMenu(null);
    try {
      switch (action) {
        case "open":
          workbench.openFile(entry);
          break;
        case "newFile":
        case "newFolder": {
          const isDirectory = action === "newFolder";
          const name = await dialog.prompt({
            title: t(isDirectory ? "workspace.newFolder" : "workspace.newFile"),
            placeholder: t(
              isDirectory ? "workspace.newFolderPlaceholder" : "workspace.newFilePlaceholder",
            ),
            confirmLabel: t("workspace.create"),
          });
          if (name) await workbench.createEntry(entry.path, name, isDirectory);
          break;
        }
        case "rename": {
          const name = await dialog.prompt({
            title: t("workspace.menu.rename"),
            defaultValue: entry.name,
            confirmLabel: t("dialog.save"),
          });
          if (name && name !== entry.name) {
            await workbench.renameEntry(entry, name);
            showToast(t("workspace.toast.renamed"), { tone: "success" });
          }
          break;
        }
        case "reveal":
          await invoke("reveal_in_folder", { path: entry.path });
          break;
        case "openExternally":
          await invoke("open_path_externally", { path: entry.path });
          break;
        case "openInVscode":
          await invoke("open_path_in_vscode", {
            path: entry.path,
            projectId: workbench.project?.id ?? null,
          });
          break;
        case "copyPath":
          await navigator.clipboard.writeText(entry.path);
          showToast(t("workspace.toast.copiedPath"), { tone: "success" });
          break;
        case "trash": {
          const hasUnsavedTab = workbench.tabs.some(
            (tab) =>
              tab.content !== tab.savedContent &&
              (tab.path === entry.path ||
                tab.path?.startsWith(`${entry.path}/`) ||
                tab.path?.startsWith(`${entry.path}\\`)),
          );
          const confirmed = await dialog.confirm({
            title: t("workspace.menu.trash"),
            message: t(
              hasUnsavedTab
                ? "workspace.deleteUnsavedConfirm"
                : entry.is_dir
                  ? "workspace.deleteFolderConfirm"
                  : "workspace.deleteFileConfirm",
            ),
            emphasis: entry.name,
            confirmLabel: t("workspace.menu.trash"),
            variant: "danger",
          });
          if (confirmed) {
            await workbench.trashEntry(entry);
            showToast(t("workspace.toast.trashed"), { tone: "success" });
          }
          break;
        }
        default:
          break;
      }
    } catch (error) {
      showToast(String(error), { tone: "error" });
    }
  };

  const menuItems: ComponentProps<typeof FileContextMenu>["items"] = menu
    ? [
        ...(!menu.entry.is_dir
          ? [
              { action: "open" as const, labelKey: "workspace.menu.open" as const },
              {
                action: "openInVscode" as const,
                labelKey: "workspace.menu.openInVscode" as const,
              },
              {
                action: "openExternally" as const,
                labelKey: "workspace.menu.openExternally" as const,
              },
            ]
          : [
              { action: "newFile" as const, labelKey: "workspace.menu.newFile" as const },
              { action: "newFolder" as const, labelKey: "workspace.menu.newFolder" as const },
              {
                action: "openInVscode" as const,
                labelKey: "workspace.menu.openInVscode" as const,
                separatorBefore: true,
              },
            ]),
        {
          action: "reveal",
          labelKey: "workspace.menu.reveal",
          separatorBefore: !menu.entry.is_dir,
        },
        { action: "copyPath", labelKey: "workspace.menu.copyPath" },
        ...(!rootPaths.has(menu.entry.path)
          ? [
              {
                action: "rename" as const,
                labelKey: "workspace.menu.rename" as const,
                separatorBefore: true,
              },
              {
                action: "trash" as const,
                labelKey: "workspace.menu.trash" as const,
                danger: true,
              },
            ]
          : []),
      ]
    : [];

  return (
    <aside
      className="project-files-panel"
      style={{ "--project-files-width": `${width}px` } as CSSProperties}
      aria-label="项目文件"
    >
      <button
        type="button"
        className="project-files-resizer"
        role="separator"
        aria-orientation="vertical"
        aria-valuemin={MIN_WIDTH}
        aria-valuemax={MAX_WIDTH}
        aria-valuenow={width}
        aria-label="调整项目文件栏宽度"
        onDoubleClick={() => updateWidth(DEFAULT_WIDTH, true)}
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          dragRef.current = { id: event.pointerId, x: event.clientX, width };
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={onPointerMove}
        onPointerUp={finishResize}
        onPointerCancel={finishResize}
        onKeyDown={onResizeKeyDown}
      />
      <header className="project-files-header">
        <div className="project-files-heading">
          <FolderOpen size={15} aria-hidden />
          <strong>{workbench.project?.name ?? "项目文件"}</strong>
        </div>
        <div className="project-files-actions">
          <button
            type="button"
            onClick={() => {
              for (const root of workbench.roots) workbench.refreshDirectory(root.path);
            }}
            aria-label="刷新项目文件"
            title="刷新"
          >
            <RefreshCw size={14} aria-hidden />
          </button>
          <button
            type="button"
            onClick={() => workbench.setPanelOpen(false)}
            aria-label="收起项目文件栏"
            title="收起"
          >
            <PanelRightClose size={15} aria-hidden />
          </button>
        </div>
      </header>
      <label className="project-files-search">
        <Search size={14} aria-hidden />
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="筛选文件…"
          aria-label="筛选项目文件"
        />
      </label>
      <div className="project-files-tree" role="tree" aria-label="项目目录结构">
        {workbench.roots.length > 0 ? (
          workbench.roots.map((root) => (
            <TreeRow
              key={root.path}
              entry={root}
              level={0}
              workbench={workbench}
              query={normalizedQuery}
              onContextMenu={openContextMenu}
            />
          ))
        ) : (
          <div className="project-files-empty">
            <Folder size={20} aria-hidden />
            <span>当前项目未配置目录</span>
          </div>
        )}
      </div>
      {menu ? (
        <FileContextMenu
          x={menu.x}
          y={menu.y}
          items={menuItems}
          onAction={(action) => void runMenuAction(action)}
          onClose={() => setMenu(null)}
          className="project-files-context-menu"
        />
      ) : null}
      {toastHost}
    </aside>
  );
}
