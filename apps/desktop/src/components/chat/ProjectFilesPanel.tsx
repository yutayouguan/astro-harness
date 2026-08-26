import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
} from "react";
import {
  ChevronRight,
  File,
  Folder,
  FolderOpen,
  PanelRightClose,
  RefreshCw,
  Search,
} from "lucide-react";
import type { FileEntryDto } from "../../types";
import type { ProjectFileWorkbench } from "../../hooks/chat/useProjectFileWorkbench";

const WIDTH_KEY = "astro.projectFiles.width";
const DEFAULT_WIDTH = 300;
const MIN_WIDTH = 236;
const MAX_WIDTH = 520;

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
};

function TreeRow({ entry, level, workbench, query }: TreeRowProps) {
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
        title={entry.path}
      >
        {entry.is_dir ? (
          <ChevronRight
            className={`project-file-chevron${expanded ? " is-open" : ""}`}
            size={14}
            aria-hidden
          />
        ) : (
          <span className="project-file-chevron-spacer" />
        )}
        {entry.is_dir ? (
          expanded ? <FolderOpen size={15} aria-hidden /> : <Folder size={15} aria-hidden />
        ) : (
          <File size={14} aria-hidden />
        )}
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
  const dragRef = useRef<{ id: number; x: number; width: number } | null>(null);

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
            />
          ))
        ) : (
          <div className="project-files-empty">
            <Folder size={20} aria-hidden />
            <span>当前项目未配置目录</span>
          </div>
        )}
      </div>
    </aside>
  );
}
