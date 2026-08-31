import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { GeneratingPreview } from "./useGeneratingPreview";
import {
  projectFileOpenPlan,
  type ProjectFilePreviewKind,
} from "../../lib/filespace/projectFilePreview";
import type { FileEntryDto, ProjectDto } from "../../types";

export type ProjectFileTab = {
  key: string;
  path: string | null;
  name: string;
  content: string;
  savedContent: string;
  loading: boolean;
  saving: boolean;
  readonly: boolean;
  transient: boolean;
  previewKind: ProjectFilePreviewKind;
  error: string | null;
};

export type ProjectFileWorkbench = {
  project: ProjectDto | null;
  panelOpen: boolean;
  setPanelOpen: (open: boolean) => void;
  togglePanel: () => void;
  entriesByDirectory: Record<string, FileEntryDto[]>;
  expandedDirectories: Set<string>;
  loadingDirectories: Set<string>;
  roots: FileEntryDto[];
  toggleDirectory: (path: string) => void;
  refreshDirectory: (path: string) => void;
  createEntry: (
    parent: string,
    name: string,
    isDirectory: boolean,
  ) => Promise<void>;
  renameEntry: (entry: FileEntryDto, newName: string) => Promise<void>;
  trashEntry: (entry: FileEntryDto) => Promise<void>;
  tabs: ProjectFileTab[];
  activeTab: ProjectFileTab | null;
  activeKey: string | null;
  setActiveKey: (key: string) => void;
  openFile: (entry: FileEntryDto) => void;
  openActiveExternally: () => Promise<void>;
  updateActiveContent: (content: string) => void;
  saveActive: () => Promise<void>;
  closeTab: (key: string) => void;
  closeAll: () => void;
};

function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

function dirname(path: string): string {
  return path.replace(/[\\/][^\\/]+$/, "");
}

function dirty(tab: ProjectFileTab): boolean {
  return !tab.readonly && tab.content !== tab.savedContent;
}

export function useProjectFileWorkbench(
  project: ProjectDto | null,
  generatingPreview: GeneratingPreview | null,
): ProjectFileWorkbench {
  const [panelOpen, setPanelOpenState] = useState(false);
  const [entriesByDirectory, setEntriesByDirectory] = useState<
    Record<string, FileEntryDto[]>
  >({});
  const entriesRef = useRef<Record<string, FileEntryDto[]>>({});
  const [expandedDirectories, setExpandedDirectories] = useState<Set<string>>(
    new Set(),
  );
  const [loadingDirectories, setLoadingDirectories] = useState<Set<string>>(
    new Set(),
  );
  const [tabs, setTabs] = useState<ProjectFileTab[]>([]);
  const [activeKey, setActiveKeyState] = useState<string | null>(null);

  const roots = useMemo<FileEntryDto[]>(
    () =>
      (project?.roots ?? []).map((path) => ({
        path,
        name: basename(path),
        is_dir: true,
        size: 0,
      })),
    [project],
  );

  const setPanelOpen = useCallback((open: boolean) => {
    setPanelOpenState(open);
  }, []);

  const loadDirectory = useCallback(
    async (path: string, force = false) => {
      if (!project || (!force && entriesRef.current[path])) return;
      setLoadingDirectories((prev) => new Set(prev).add(path));
      try {
        const entries = await invoke<FileEntryDto[]>("project_list_files", {
          projectId: project.id,
          path,
        });
        setEntriesByDirectory((prev) => {
          const next = { ...prev, [path]: entries };
          entriesRef.current = next;
          return next;
        });
      } catch {
        setEntriesByDirectory((prev) => {
          const next = { ...prev, [path]: [] };
          entriesRef.current = next;
          return next;
        });
      } finally {
        setLoadingDirectories((prev) => {
          const next = new Set(prev);
          next.delete(path);
          return next;
        });
      }
    },
    [project],
  );

  useEffect(() => {
    setEntriesByDirectory({});
    entriesRef.current = {};
    setExpandedDirectories(new Set(roots.map((root) => root.path)));
    setTabs([]);
    setActiveKeyState(null);
    if (panelOpen) {
      for (const root of roots) void loadDirectory(root.path);
    }
    // `loadDirectory` intentionally excluded: changing its identity after a directory load
    // must not reset the editor. Project identity is the lifecycle boundary here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project?.id]);

  useEffect(() => {
    if (!panelOpen) return;
    for (const root of roots) void loadDirectory(root.path);
  }, [loadDirectory, panelOpen, roots]);

  const toggleDirectory = useCallback(
    (path: string) => {
      setExpandedDirectories((prev) => {
        const next = new Set(prev);
        if (next.has(path)) next.delete(path);
        else next.add(path);
        return next;
      });
      void loadDirectory(path);
    },
    [loadDirectory],
  );

  const renameEntry = useCallback(
    async (entry: FileEntryDto, newName: string) => {
      if (!project) return;
      const renamed = await invoke<FileEntryDto>("project_rename_path", {
        projectId: project.id,
        path: entry.path,
        newName,
      });
      const oldPrefix = `${entry.path}/`;
      const oldPrefixWindows = `${entry.path}\\`;
      const remapPath = (path: string) =>
        path === entry.path
          ? renamed.path
          : path.startsWith(oldPrefix)
            ? `${renamed.path}/${path.slice(oldPrefix.length)}`
            : path.startsWith(oldPrefixWindows)
              ? `${renamed.path}\\${path.slice(oldPrefixWindows.length)}`
              : path;

      setTabs((prev) =>
        prev.map((tab) => {
          if (!tab.path) return tab;
          const path = remapPath(tab.path);
          const plan = projectFileOpenPlan(basename(path));
          return path === tab.path
            ? tab
            : {
                ...tab,
                key: path,
                path,
                name: basename(path),
                readonly: plan.readonly,
                previewKind: plan.kind,
              };
        }),
      );
      setActiveKeyState((current) => (current ? remapPath(current) : current));
      setExpandedDirectories((prev) => new Set([...prev].map(remapPath)));
      setEntriesByDirectory((prev) => {
        const next: Record<string, FileEntryDto[]> = {};
        for (const [directory, entries] of Object.entries(prev)) {
          next[remapPath(directory)] = entries.map((item) => ({
            ...item,
            path: remapPath(item.path),
            name: item.path === entry.path ? renamed.name : item.name,
          }));
        }
        entriesRef.current = next;
        return next;
      });
      const parent = dirname(renamed.path);
      if (parent) await loadDirectory(parent, true);
    },
    [loadDirectory, project],
  );

  const trashEntry = useCallback(
    async (entry: FileEntryDto) => {
      if (!project) return;
      await invoke("project_trash_paths", {
        projectId: project.id,
        paths: [entry.path],
      });
      const contains = (path: string | null) =>
        path === entry.path ||
        Boolean(path?.startsWith(`${entry.path}/`)) ||
        Boolean(path?.startsWith(`${entry.path}\\`));
      setTabs((prev) => {
        const next = prev.filter((tab) => !contains(tab.path));
        setActiveKeyState((current) =>
          contains(current) ? (next[next.length - 1]?.key ?? null) : current,
        );
        return next;
      });
      setExpandedDirectories(
        (prev) => new Set([...prev].filter((path) => !contains(path))),
      );
      const parent = dirname(entry.path);
      if (parent) await loadDirectory(parent, true);
    },
    [loadDirectory, project],
  );

  const openFile = useCallback(
    (entry: FileEntryDto) => {
      if (!project || entry.is_dir) return;
      const plan = projectFileOpenPlan(entry.name);
      setPanelOpen(true);
      setActiveKeyState(entry.path);
      setTabs((prev) => {
        if (prev.some((tab) => tab.key === entry.path)) return prev;
        return [
          ...prev,
          {
            key: entry.path,
            path: entry.path,
            name: entry.name,
            content: "",
            savedContent: "",
            loading: plan.readAsText,
            saving: false,
            readonly: plan.readonly,
            transient: false,
            previewKind: plan.kind,
            error: null,
          },
        ];
      });
      if (!plan.readAsText) return;
      void invoke<string>("project_read_file", {
        projectId: project.id,
        path: entry.path,
      })
        .then((content) => {
          setTabs((prev) =>
            prev.map((tab) =>
              tab.key === entry.path
                ? {
                    ...tab,
                    content,
                    savedContent: content,
                    loading: false,
                    error: null,
                  }
                : tab,
            ),
          );
        })
        .catch((error) => {
          setTabs((prev) =>
            prev.map((tab) =>
              tab.key === entry.path
                ? { ...tab, loading: false, error: String(error) }
                : tab,
            ),
          );
        });
    },
    [project, setPanelOpen],
  );

  const createEntry = useCallback(
    async (parent: string, name: string, isDirectory: boolean) => {
      if (!project) return;
      const entry = await invoke<FileEntryDto>(
        isDirectory ? "project_create_directory" : "project_create_file",
        { projectId: project.id, parent, name },
      );
      setExpandedDirectories((prev) => new Set(prev).add(parent));
      await loadDirectory(parent, true);
      if (!isDirectory) openFile(entry);
    },
    [loadDirectory, openFile, project],
  );

  useEffect(() => {
    if (!generatingPreview) return;
    const key = `preview:${generatingPreview.toolKey}`;
    setPanelOpen(true);
    setActiveKeyState(key);
    setTabs((prev) => {
      const next: ProjectFileTab = {
        key,
        path: generatingPreview.path,
        name: generatingPreview.filename ?? "生成中的文件",
        content: generatingPreview.content,
        savedContent: generatingPreview.content,
        loading: false,
        saving: false,
        readonly: generatingPreview.status === "streaming",
        transient: true,
        previewKind: "text",
        error: null,
      };
      const index = prev.findIndex((tab) => tab.key === key);
      if (index < 0) return [...prev, next];
      return prev.map((tab, i) => (i === index ? next : tab));
    });

    if (
      generatingPreview.status !== "done" ||
      !generatingPreview.path ||
      !project
    )
      return;
    const path = generatingPreview.path;
    const name = basename(path);
    const plan = projectFileOpenPlan(name);
    if (!plan.readAsText) {
      setTabs((prev) =>
        prev.map((tab) =>
          tab.key === key
            ? {
                ...tab,
                key: path,
                path,
                name,
                content: "",
                savedContent: "",
                readonly: plan.readonly,
                transient: false,
                previewKind: plan.kind,
              }
            : tab,
        ),
      );
      setActiveKeyState((current) => (current === key ? path : current));
      const parent = dirname(path);
      if (parent) void loadDirectory(parent, true);
      return;
    }
    void invoke<string>("project_read_file", { projectId: project.id, path })
      .then((content) => {
        setTabs((prev) =>
          prev.map((tab) =>
            tab.key === key
              ? {
                  ...tab,
                  key: path,
                  path,
                  name: basename(path),
                  content,
                  savedContent: content,
                  readonly: false,
                  transient: false,
                  previewKind: plan.kind,
                }
              : tab,
          ),
        );
        setActiveKeyState((current) => (current === key ? path : current));
        const parent = path.replace(/[\\/][^\\/]+$/, "");
        if (parent) void loadDirectory(parent, true);
      })
      .catch(() => {
        // Tool may write outside the selected project (for example an isolated worktree).
        // Keep the completed snapshot readable instead of discarding it.
      });
  }, [generatingPreview, loadDirectory, project, setPanelOpen]);

  const activeTab = useMemo(
    () => tabs.find((tab) => tab.key === activeKey) ?? null,
    [activeKey, tabs],
  );

  const updateActiveContent = useCallback(
    (content: string) => {
      if (!activeKey) return;
      setTabs((prev) =>
        prev.map((tab) =>
          tab.key === activeKey && !tab.readonly ? { ...tab, content } : tab,
        ),
      );
    },
    [activeKey],
  );

  const saveActive = useCallback(async () => {
    if (!project || !activeTab?.path || activeTab.readonly || activeTab.loading)
      return;
    const key = activeTab.key;
    setTabs((prev) =>
      prev.map((tab) => (tab.key === key ? { ...tab, saving: true } : tab)),
    );
    try {
      await invoke("project_write_file", {
        projectId: project.id,
        path: activeTab.path,
        content: activeTab.content,
      });
      setTabs((prev) =>
        prev.map((tab) =>
          tab.key === key
            ? { ...tab, saving: false, savedContent: tab.content, error: null }
            : tab,
        ),
      );
    } catch (error) {
      setTabs((prev) =>
        prev.map((tab) =>
          tab.key === key
            ? { ...tab, saving: false, error: String(error) }
            : tab,
        ),
      );
    }
  }, [activeTab, project]);

  const openActiveExternally = useCallback(async () => {
    if (!project || !activeTab?.path) return;
    await invoke("project_open_path_externally", {
      projectId: project.id,
      path: activeTab.path,
    });
  }, [activeTab?.path, project]);

  const closeTab = useCallback(
    (key: string) => {
      const target = tabs.find((tab) => tab.key === key);
      if (
        target &&
        dirty(target) &&
        !window.confirm(`“${target.name}”尚未保存，仍要关闭吗？`)
      ) {
        return;
      }
      setTabs((prev) => {
        const index = prev.findIndex((tab) => tab.key === key);
        const next = prev.filter((tab) => tab.key !== key);
        if (activeKey === key) {
          setActiveKeyState(
            next[Math.max(0, index - 1)]?.key ?? next[0]?.key ?? null,
          );
        }
        return next;
      });
    },
    [activeKey, tabs],
  );

  const closeAll = useCallback(() => {
    if (
      tabs.some(dirty) &&
      !window.confirm("仍有未保存文件，确定关闭全部文件吗？")
    )
      return;
    setTabs([]);
    setActiveKeyState(null);
  }, [tabs]);

  return {
    project,
    panelOpen,
    setPanelOpen,
    togglePanel: () => setPanelOpen(!panelOpen),
    entriesByDirectory,
    expandedDirectories,
    loadingDirectories,
    roots,
    toggleDirectory,
    refreshDirectory: (path) => void loadDirectory(path, true),
    createEntry,
    renameEntry,
    trashEntry,
    tabs,
    activeTab,
    activeKey,
    setActiveKey: setActiveKeyState,
    openFile,
    openActiveExternally,
    updateActiveContent,
    saveActive,
    closeTab,
    closeAll,
  };
}
