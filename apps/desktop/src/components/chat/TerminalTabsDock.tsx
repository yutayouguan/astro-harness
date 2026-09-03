import { invoke } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XtermTerminal } from "@xterm/xterm";
import {
  Bot,
  createLucideIcon,
  ExternalLink,
  PanelBottomClose,
  Plus,
  RefreshCw,
  Trash2,
  UserRound,
} from "lucide-react";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import "@xterm/xterm/css/xterm.css";

import { useI18n } from "../../i18n/LocaleContext";
import {
  readTerminalSettings,
  subscribeTerminalSettings,
  type TerminalSettings,
} from "../../lib/terminal/terminalSettings";
import {
  MAX_TERMINAL_TABS,
  createTerminalClientId,
  readTerminalTabLayout,
  saveTerminalTabLayout,
  type TerminalTab,
} from "../../lib/terminal/terminalTabs";

type TerminalSessionDto = {
  id: number;
  scope: string;
  cwd: string;
  running: boolean;
  exitCode?: number | null;
  baseCursor: number;
  endCursor: number;
};

type TerminalReadResultDto = {
  id: number;
  data: number[];
  nextCursor: number;
  dropped: boolean;
  running: boolean;
  exitCode?: number | null;
};

type Props = {
  open?: boolean;
  projectId: string;
  projectName: string;
  projectRoot: string;
  onClose: () => void;
};

type TerminalPaneProps = {
  clientId: string;
  visible: boolean;
  session: TerminalSessionDto;
  settings: TerminalSettings;
  startCursor: number;
  clearGeneration: number;
  onSessionProgress: (clientId: string, session: TerminalSessionDto) => void;
  onError: (message: string | null) => void;
};

const HEIGHT_KEY_PREFIX = "astro.terminalDock.height.";
const DEFAULT_TERMINAL_DOCK_HEIGHT = 260;
const MIN_TERMINAL_DOCK_HEIGHT = 160;
const MAX_TERMINAL_DOCK_HEIGHT = 720;
const RESIZE_KEYBOARD_STEP = 24;
const WRITE_CHUNK_BYTES = 32 * 1024;
const SCROLLBAR_WIDTH = 4;
const TERMINAL_START_TIMEOUT_MS = 15_000;
const READ_RETRY_DELAYS = [500, 1_000, 2_000, 4_000];
const OPEN_RETRY_DELAYS = [1_000, 2_000, 4_000];

const Broom = createLucideIcon("Broom", [
  ["path", { d: "M13.5 10.5 22 2", key: "broom-handle" }],
  [
    "path",
    {
      d: "M14.734 13.841a2 2 0 0 0-.314-2.42L12.58 9.58a2 2 0 0 0-2.421-.314l-7.657 4.461A1 1 0 0 0 2.3 15.3l6.403 6.403a1 1 0 0 0 1.571-.204z",
      key: "broom-head",
    },
  ],
  ["path", { d: "m5 18 2-2", key: "broom-bristle-short" }],
  ["path", { d: "m7.699 10.7 5.602 5.601", key: "broom-bristle-long" }],
]);

function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message: string,
): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = window.setTimeout(() => reject(message), timeoutMs);
    promise.then(resolve, reject).finally(() => window.clearTimeout(timer));
  });
}

function initialHeight(projectId: string): number {
  if (typeof window === "undefined") return DEFAULT_TERMINAL_DOCK_HEIGHT;
  try {
    const stored = Number(
      localStorage.getItem(`${HEIGHT_KEY_PREFIX}${projectId}`),
    );
    return Number.isFinite(stored) && stored >= MIN_TERMINAL_DOCK_HEIGHT
      ? clampTerminalDockHeight(stored)
      : DEFAULT_TERMINAL_DOCK_HEIGHT;
  } catch {
    return DEFAULT_TERMINAL_DOCK_HEIGHT;
  }
}

function maxTerminalDockHeight(): number {
  if (typeof window === "undefined") return MAX_TERMINAL_DOCK_HEIGHT;
  return Math.max(
    MIN_TERMINAL_DOCK_HEIGHT,
    Math.min(
      MAX_TERMINAL_DOCK_HEIGHT,
      Math.floor(window.innerHeight * 0.65),
    ),
  );
}

function clampTerminalDockHeight(height: number): number {
  return Math.max(
    MIN_TERMINAL_DOCK_HEIGHT,
    Math.min(maxTerminalDockHeight(), Math.round(height)),
  );
}

function terminalTheme() {
  const styles = getComputedStyle(document.documentElement);
  const read = (name: string, fallback: string) =>
    styles.getPropertyValue(name).trim() || fallback;
  return {
    background: read("--sidebar-bg", "#111318"),
    foreground: read("--ink", "#e8eaf0"),
    cursor: read("--tone", "#7aa2f7"),
    selectionBackground: read("--tone-soft", "rgba(122, 162, 247, 0.28)"),
  };
}

function TerminalPane({
  clientId,
  visible,
  session,
  settings,
  startCursor,
  clearGeneration,
  onSessionProgress,
  onError,
}: TerminalPaneProps) {
  const { t } = useI18n();
  const hostRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XtermTerminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const visibleRef = useRef(visible);
  visibleRef.current = visible;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    let disposed = false;
    let cursor = Math.max(session.baseCursor, startCursor);
    let pendingInput = "";
    let writing = false;

    const terminal = new XtermTerminal({
      allowProposedApi: false,
      convertEol: false,
      cursorBlink: settings.cursorBlink,
      cursorStyle: settings.cursorStyle,
      fontFamily: settings.fontFamily,
      fontSize: settings.fontSize,
      lineHeight: settings.lineHeight,
      overviewRuler: { width: SCROLLBAR_WIDTH },
      scrollback: settings.scrollback,
      theme: terminalTheme(),
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(host);
    xtermRef.current = terminal;
    fitRef.current = fit;
    if (visibleRef.current) fit.fit();

    const flushInput = async () => {
      if (writing || !pendingInput || disposed) return;
      writing = true;
      try {
        while (pendingInput && !disposed) {
          const data = pendingInput;
          pendingInput = "";
          const encoded = new TextEncoder().encode(data);
          for (
            let offset = 0;
            offset < encoded.length && !disposed;
            offset += WRITE_CHUNK_BYTES
          ) {
            await invoke("terminal_write", {
              request: {
                id: session.id,
                data: Array.from(
                  encoded.subarray(offset, offset + WRITE_CHUNK_BYTES),
                ),
              },
            });
          }
        }
      } catch (reason) {
        if (!disposed) onError(String(reason));
      } finally {
        writing = false;
        if (pendingInput && !disposed) void flushInput();
      }
    };

    const inputDisposable = terminal.onData((data) => {
      pendingInput += data;
      void flushInput();
    });
    let resizeTimer: number | null = null;
    const resizeDisposable = terminal.onResize(({ cols, rows }) => {
      if (resizeTimer !== null) window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(() => {
        resizeTimer = null;
        if (disposed) return;
        void invoke("terminal_resize", {
          request: { id: session.id, cols, rows },
        }).catch(() => undefined);
      }, 60);
    });
    let fitFrame: number | null = null;
    const observer = new ResizeObserver(() => {
      if (fitFrame !== null) return;
      fitFrame = window.requestAnimationFrame(() => {
        fitFrame = null;
        if (!disposed && visibleRef.current) fit.fit();
      });
    });
    observer.observe(host);
    const themeObserver = new MutationObserver(() => {
      terminal.options.theme = terminalTheme();
    });
    themeObserver.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class", "data-theme", "style"],
    });

    const readOutput = async () => {
      let retries = 0;
      while (!disposed) {
        try {
          const result = await invoke<TerminalReadResultDto>("terminal_read", {
            request: {
              id: session.id,
              cursor,
              maxBytes: 64 * 1024,
              waitMs: 5_000,
            },
          });
          if (disposed) return;
          retries = 0;
          cursor = result.nextCursor;
          if (result.dropped) {
            terminal.writeln(`\r\n[${t("chat.terminal.outputTruncated")}]`);
          }
          if (result.data.length > 0)
            terminal.write(Uint8Array.from(result.data));
          onSessionProgress(clientId, {
            ...session,
            running: result.running,
            exitCode: result.exitCode,
            endCursor: result.nextCursor,
          });
          if (!result.running) {
            terminal.writeln(
              `\r\n[${t("chat.terminal.exited", { code: String(result.exitCode ?? "-") })}]`,
            );
            break;
          }
        } catch {
          if (disposed) return;
          const delay = READ_RETRY_DELAYS[retries];
          if (delay === undefined) {
            onError(t("chat.terminal.tab.readFailed"));
            return;
          }
          retries++;
          await new Promise((r) => setTimeout(r, delay));
        }
      }
    };

    void readOutput();
    terminal.focus();
    return () => {
      disposed = true;
      observer.disconnect();
      themeObserver.disconnect();
      if (fitFrame !== null) window.cancelAnimationFrame(fitFrame);
      if (resizeTimer !== null) window.clearTimeout(resizeTimer);
      inputDisposable.dispose();
      resizeDisposable.dispose();
      terminal.dispose();
      xtermRef.current = null;
      fitRef.current = null;
    };
  }, [session.id, t]);

  useEffect(() => {
    const terminal = xtermRef.current;
    if (!terminal) return;
    terminal.options.fontFamily = settings.fontFamily;
    terminal.options.fontSize = settings.fontSize;
    terminal.options.lineHeight = settings.lineHeight;
    terminal.options.scrollback = settings.scrollback;
    terminal.options.cursorStyle = settings.cursorStyle;
    terminal.options.cursorBlink = settings.cursorBlink;
    if (visibleRef.current)
      window.requestAnimationFrame(() => fitRef.current?.fit());
  }, [settings]);

  useEffect(() => {
    if (visible) window.requestAnimationFrame(() => fitRef.current?.fit());
  }, [visible]);

  useEffect(() => {
    if (clearGeneration > 0) xtermRef.current?.clear();
  }, [clearGeneration]);

  return <div ref={hostRef} className="terminal-dock-screen" />;
}

export default function TerminalTabsDock({
  open = true,
  projectId,
  projectName,
  projectRoot,
  onClose,
}: Props) {
  const { t } = useI18n();
  const initialSettings = useMemo(() => readTerminalSettings(), []);
  const initialLayout = useMemo(
    () =>
      readTerminalTabLayout(
        projectId,
        projectRoot,
        t("chat.terminal.tab.user"),
        t("chat.terminal.tab.ai"),
        initialSettings.executionMode,
      ),
    [initialSettings.executionMode, projectId, projectRoot, t],
  );
  const [tabs, setTabs] = useState(initialLayout.tabs);
  const [activeClientId, setActiveClientId] = useState(
    initialLayout.activeClientId,
  );
  const [sessions, setSessions] = useState<Record<string, TerminalSessionDto>>(
    {},
  );
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [closing, setClosing] = useState<Set<string>>(() => new Set());
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [clearState, setClearState] = useState<
    Record<string, { cursor: number; generation: number }>
  >({});
  const [height, setHeight] = useState(() => initialHeight(projectId));
  const [resizing, setResizing] = useState(false);
  const [settings, setSettings] = useState(initialSettings);
  const dockRef = useRef<HTMLElement>(null);
  const heightRef = useRef(height);
  const pendingHeightRef = useRef(height);
  const resizeFrameRef = useRef<number | null>(null);
  const tabsRef = useRef(tabs);
  const sessionsRef = useRef(sessions);
  const cursorByClientRef = useRef<Record<string, number>>({});
  const openingRef = useRef(new Set<string>());
  const mutatingRef = useRef(new Set<string>());
  const removedTokensRef = useRef(new Set<string>());
  const resizeDragCleanupRef = useRef<(() => void) | null>(null);

  tabsRef.current = tabs;
  sessionsRef.current = sessions;
  heightRef.current = height;
  const activeTab =
    tabs.find((tab) => tab.clientId === activeClientId) ?? tabs[0];
  const activeSession = activeTab ? sessions[activeTab.clientId] : undefined;
  const activeError = activeTab ? errors[activeTab.clientId] : undefined;

  useEffect(() => subscribeTerminalSettings(setSettings), []);

  useEffect(() => {
    saveTerminalTabLayout(projectId, { tabs, activeClientId });
  }, [activeClientId, projectId, tabs]);

  const openTab = useCallback(
    async (tab: TerminalTab) => {
      if (openingRef.current.has(tab.clientId)) return;
      openingRef.current.add(tab.clientId);
      const wasRemoved = () =>
        removedTokensRef.current.has(tab.clientId) ||
        !tabsRef.current.some((c) => c.clientId === tab.clientId);
      let lastError: unknown;
      for (let attempt = 0; attempt <= OPEN_RETRY_DELAYS.length; attempt++) {
        if (wasRemoved()) break;
        if (attempt > 0) {
          await new Promise((r) =>
            setTimeout(r, OPEN_RETRY_DELAYS[attempt - 1]),
          );
          if (wasRemoved()) break;
        }
        try {
          const opened = await withTimeout(
            invoke<TerminalSessionDto>("terminal_open", {
              request: {
                scope: projectRoot,
                cwd: tab.cwd,
                cols: 120,
                rows: 32,
                executionMode: tab.executionMode,
                clientToken: tab.clientId,
                agentDefault: tab.agentDefault,
              },
            }),
            TERMINAL_START_TIMEOUT_MS,
            t("chat.terminal.tab.startTimeout"),
          );
          if (wasRemoved()) {
            await invoke("terminal_close", { id: opened.id }).catch(
              () => undefined,
            );
            lastError = undefined;
            break;
          }
          setSessions((current) => ({ ...current, [tab.clientId]: opened }));
          cursorByClientRef.current[tab.clientId] = opened.endCursor;
          setErrors((current) => {
            const next = { ...current };
            delete next[tab.clientId];
            return next;
          });
          lastError = undefined;
          break;
        } catch (reason) {
          lastError = reason;
        }
      }
      if (lastError && !wasRemoved()) {
        setErrors((current) => ({
          ...current,
          [tab.clientId]: String(lastError),
        }));
      }
      openingRef.current.delete(tab.clientId);
      if (removedTokensRef.current.has(tab.clientId)) {
        removedTokensRef.current.delete(tab.clientId);
      }
    },
    [projectRoot, t],
  );

  useEffect(() => {
    if (activeTab && !activeSession && !activeError) void openTab(activeTab);
  }, [activeError, activeSession, activeTab, openTab]);

  const createTab = useCallback(() => {
    if (tabsRef.current.length >= MAX_TERMINAL_TABS) {
      setErrors((current) => ({
        ...current,
        [activeClientId]: t("chat.terminal.tab.limit", {
          count: String(MAX_TERMINAL_TABS),
        }),
      }));
      return;
    }
    const tab: TerminalTab = {
      clientId: createTerminalClientId(),
      title: t("chat.terminal.tab.number", {
        number: String(tabsRef.current.length + 1),
      }),
      cwd: projectRoot,
      executionMode: settings.executionMode,
      agentDefault: false,
    };
    setTabs((current) => [...current, tab]);
    setActiveClientId(tab.clientId);
  }, [activeClientId, projectRoot, settings.executionMode, t]);

  const closeTab = useCallback(
    async (clientId: string) => {
      if (mutatingRef.current.has(clientId)) return;
      const tab = tabsRef.current.find(
        (candidate) => candidate.clientId === clientId,
      );
      if (!tab) return;
      mutatingRef.current.add(clientId);
      removedTokensRef.current.add(clientId);
      setClosing((current) => new Set(current).add(clientId));
      try {
        const session = sessionsRef.current[clientId];
        if (session) await invoke("terminal_close", { id: session.id });
        let nextTabs = tabsRef.current.filter(
          (candidate) => candidate.clientId !== clientId,
        );
        if (tab.agentDefault) {
          nextTabs = [
            ...nextTabs,
            {
              clientId: createTerminalClientId(),
              title: t("chat.terminal.tab.ai"),
              cwd: projectRoot,
              executionMode: "project",
              agentDefault: true,
            },
          ];
        }
        setTabs(nextTabs);
        setSessions((current) => {
          const next = { ...current };
          delete next[clientId];
          return next;
        });
        setClearState((current) => {
          const next = { ...current };
          delete next[clientId];
          return next;
        });
        delete cursorByClientRef.current[clientId];
        if (activeClientId === clientId) {
          const oldIndex = tabsRef.current.findIndex(
            (candidate) => candidate.clientId === clientId,
          );
          setActiveClientId(
            nextTabs[Math.min(oldIndex, nextTabs.length - 1)].clientId,
          );
        }
        if (!openingRef.current.has(clientId))
          removedTokensRef.current.delete(clientId);
      } catch (reason) {
        removedTokensRef.current.delete(clientId);
        setErrors((current) => ({ ...current, [clientId]: String(reason) }));
      } finally {
        mutatingRef.current.delete(clientId);
        setClosing((current) => {
          const next = new Set(current);
          next.delete(clientId);
          return next;
        });
      }
    },
    [activeClientId, projectRoot, t],
  );

  const restartActive = useCallback(async () => {
    if (!activeTab || mutatingRef.current.has(activeTab.clientId)) return;
    mutatingRef.current.add(activeTab.clientId);
    openingRef.current.add(activeTab.clientId);
    const session = sessionsRef.current[activeTab.clientId];
    if (!session) {
      setErrors((current) => {
        const next = { ...current };
        delete next[activeTab.clientId];
        return next;
      });
      openingRef.current.delete(activeTab.clientId);
      await openTab(activeTab);
      mutatingRef.current.delete(activeTab.clientId);
      return;
    }
    setClosing((current) => new Set(current).add(activeTab.clientId));
    try {
      await invoke("terminal_close", { id: session.id }).catch(() => undefined);
      setSessions((current) => {
        const next = { ...current };
        delete next[activeTab.clientId];
        return next;
      });
      delete cursorByClientRef.current[activeTab.clientId];
      setErrors((current) => {
        const next = { ...current };
        delete next[activeTab.clientId];
        return next;
      });
      openingRef.current.delete(activeTab.clientId);
      await openTab(activeTab);
    } catch (reason) {
      openingRef.current.delete(activeTab.clientId);
      setErrors((current) => ({
        ...current,
        [activeTab.clientId]: String(reason),
      }));
    } finally {
      mutatingRef.current.delete(activeTab.clientId);
      setClosing((current) => {
        const next = new Set(current);
        next.delete(activeTab.clientId);
        return next;
      });
    }
  }, [activeTab, openTab]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.repeat || event.isComposing) return;
      if (!(event.metaKey || event.ctrlKey) || !event.shiftKey || event.altKey)
        return;
      const key = event.key.toLowerCase();
      if (key === "t") {
        event.preventDefault();
        createTab();
      } else if (key === "w" && activeTab) {
        event.preventDefault();
        void closeTab(activeTab.clientId);
      } else if ((event.key === "[" || event.key === "]") && tabs.length > 1) {
        event.preventDefault();
        const current = tabs.findIndex(
          (tab) => tab.clientId === activeTab?.clientId,
        );
        const delta = event.key === "]" ? 1 : -1;
        setActiveClientId(
          tabs[(current + delta + tabs.length) % tabs.length].clientId,
        );
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [activeTab, closeTab, createTab, open, tabs]);

  const commitRename = useCallback(
    (clientId: string) => {
      const title = renameDraft.trim().slice(0, 60);
      if (title) {
        setTabs((current) =>
          current.map((tab) =>
            tab.clientId === clientId ? { ...tab, title } : tab,
          ),
        );
      }
      setRenaming(null);
    },
    [renameDraft],
  );

  const beginResize = useCallback(
    (event: ReactPointerEvent<HTMLButtonElement>) => {
      if (event.button !== 0) return;
      event.preventDefault();
      const startY = event.clientY;
      const startHeight = heightRef.current;
      resizeDragCleanupRef.current?.();
      setResizing(true);

      const paintHeight = () => {
        resizeFrameRef.current = null;
        heightRef.current = pendingHeightRef.current;
        dockRef.current?.style.setProperty(
          "--terminal-dock-height",
          `${pendingHeightRef.current}px`,
        );
      };
      const move = (moveEvent: PointerEvent) => {
        pendingHeightRef.current = clampTerminalDockHeight(
          startHeight + startY - moveEvent.clientY,
        );
        if (resizeFrameRef.current === null) {
          resizeFrameRef.current = window.requestAnimationFrame(paintHeight);
        }
      };
      const stopListening = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", finish);
        window.removeEventListener("pointercancel", finish);
        window.removeEventListener("blur", finish);
        if (resizeFrameRef.current !== null) {
          window.cancelAnimationFrame(resizeFrameRef.current);
          resizeFrameRef.current = null;
        }
        resizeDragCleanupRef.current = null;
      };
      const finish = () => {
        const finalHeight = pendingHeightRef.current;
        stopListening();
        heightRef.current = finalHeight;
        setHeight(finalHeight);
        setResizing(false);
      };
      pendingHeightRef.current = startHeight;
      resizeDragCleanupRef.current = stopListening;
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", finish, { once: true });
      window.addEventListener("pointercancel", finish, { once: true });
      window.addEventListener("blur", finish, { once: true });
    },
    [],
  );

  const resizeKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLButtonElement>) => {
      let nextHeight: number | null = null;
      if (event.key === "ArrowUp")
        nextHeight = heightRef.current + RESIZE_KEYBOARD_STEP;
      else if (event.key === "ArrowDown")
        nextHeight = heightRef.current - RESIZE_KEYBOARD_STEP;
      else if (event.key === "Home") nextHeight = MIN_TERMINAL_DOCK_HEIGHT;
      else if (event.key === "End") nextHeight = maxTerminalDockHeight();
      if (nextHeight === null) return;
      event.preventDefault();
      setHeight(clampTerminalDockHeight(nextHeight));
    },
    [],
  );

  useEffect(() => () => resizeDragCleanupRef.current?.(), []);
  useEffect(() => {
    const timer = window.setTimeout(() => {
      try {
        localStorage.setItem(
          `${HEIGHT_KEY_PREFIX}${projectId}`,
          String(Math.round(height)),
        );
      } catch {
        // Resizing remains functional when WebView storage is unavailable.
      }
    }, 120);
    return () => window.clearTimeout(timer);
  }, [height, projectId]);

  const updateSessionProgress = useCallback(
    (clientId: string, next: TerminalSessionDto) => {
      if (sessionsRef.current[clientId]?.id !== next.id) return;
      cursorByClientRef.current[clientId] = next.endCursor;
      setSessions((current) => {
        const previous = current[clientId];
        if (!previous || previous.id !== next.id) return current;
        if (
          previous.running === next.running &&
          previous.exitCode === next.exitCode
        ) {
          return current;
        }
        return { ...current, [clientId]: next };
      });
    },
    [],
  );

  const renameKeyDown = (
    event: ReactKeyboardEvent<HTMLInputElement>,
    clientId: string,
  ) => {
    if (event.key === "Enter") commitRename(clientId);
    if (event.key === "Escape") setRenaming(null);
  };

  return (
    <section
      ref={dockRef}
      className={`terminal-dock terminal-tabs-dock${open ? " is-open" : ""}${resizing ? " is-resizing" : ""}`}
      style={{ "--terminal-dock-height": `${height}px` } as CSSProperties}
      aria-label={t("chat.terminal.title")}
      aria-hidden={!open}
    >
      <button
        type="button"
        className="terminal-dock-resizer"
        onPointerDown={beginResize}
        onDoubleClick={() => setHeight(DEFAULT_TERMINAL_DOCK_HEIGHT)}
        onKeyDown={resizeKeyDown}
        role="separator"
        aria-orientation="horizontal"
        aria-valuemin={MIN_TERMINAL_DOCK_HEIGHT}
        aria-valuemax={maxTerminalDockHeight()}
        aria-valuenow={Math.round(height)}
        aria-label={t("chat.terminal.resize")}
      />
      <header className="terminal-dock-header">
        <div
          className="terminal-tab-list"
          role="tablist"
          aria-label={projectName}
        >
          {tabs.map((tab) => {
            const tabSession = sessions[tab.clientId];
            const active = tab.clientId === activeTab?.clientId;
            return (
              <div
                key={tab.clientId}
                className={`terminal-tab ${active ? "is-active" : ""}`}
              >
                {renaming === tab.clientId ? (
                  <input
                    className="terminal-tab-rename"
                    autoFocus
                    value={renameDraft}
                    aria-label={t("chat.terminal.tab.rename")}
                    onChange={(event) => setRenameDraft(event.target.value)}
                    onBlur={() => commitRename(tab.clientId)}
                    onKeyDown={(event) => renameKeyDown(event, tab.clientId)}
                  />
                ) : (
                  <button
                    type="button"
                    className="terminal-tab-main"
                    role="tab"
                    aria-selected={active}
                    tabIndex={active ? 0 : -1}
                    onClick={() => setActiveClientId(tab.clientId)}
                    onDoubleClick={() => {
                      setRenameDraft(tab.title);
                      setRenaming(tab.clientId);
                    }}
                  >
                    <span
                      className={`terminal-dock-status ${tabSession?.running ? "is-running" : ""}`}
                      aria-hidden
                    />
                    {tab.agentDefault ? (
                      <Bot size={13} aria-hidden />
                    ) : (
                      <UserRound size={13} aria-hidden />
                    )}
                    <span>{tab.title}</span>
                  </button>
                )}
                <button
                  type="button"
                  className="terminal-tab-close"
                  disabled={closing.has(tab.clientId)}
                  onClick={(event) => {
                    event.stopPropagation();
                    void closeTab(tab.clientId);
                  }}
                  title={t("chat.terminal.tab.close")}
                  aria-label={t("chat.terminal.tab.closeNamed", {
                    name: tab.title,
                  })}
                >
                  <Trash2 size={12} aria-hidden />
                </button>
              </div>
            );
          })}
          <button
            type="button"
            className="terminal-tab-add"
            onClick={createTab}
            disabled={tabs.length >= MAX_TERMINAL_TABS}
            title={t("chat.terminal.tab.new")}
            aria-label={t("chat.terminal.tab.new")}
          >
            <Plus size={14} aria-hidden />
          </button>
        </div>
        <div className="terminal-dock-actions">
          {activeError ? (
            <span className="terminal-dock-error">{activeError}</span>
          ) : null}
          {activeTab ? (
            <span className="terminal-dock-shared">
              {activeTab.agentDefault
                ? t("chat.terminal.tab.aiManaged")
                : t(
                    activeTab.executionMode === "system"
                      ? "chat.terminal.mode.system"
                      : "chat.terminal.mode.project",
                  )}
            </span>
          ) : null}
          <button
            type="button"
            onClick={() => {
              if (!activeTab) return;
              void invoke("terminal_open_external", {
                cwd: activeTab.cwd,
              }).catch((reason) =>
                setErrors((current) => ({
                  ...current,
                  [activeTab.clientId]: String(reason),
                })),
              );
            }}
            title={t("chat.terminal.external")}
            aria-label={t("chat.terminal.external")}
          >
            <ExternalLink size={14} aria-hidden />
          </button>
          <button
            type="button"
            onClick={() => {
              if (!activeTab || !activeSession) return;
              setClearState((current) => ({
                ...current,
                [activeTab.clientId]: {
                  cursor:
                    cursorByClientRef.current[activeTab.clientId] ??
                    activeSession.endCursor,
                  generation:
                    (current[activeTab.clientId]?.generation ?? 0) + 1,
                },
              }));
            }}
            title={t("chat.terminal.clear")}
            aria-label={t("chat.terminal.clear")}
          >
            <Broom size={14} aria-hidden />
          </button>
          <button
            type="button"
            onClick={() => void restartActive()}
            disabled={!activeTab || closing.has(activeTab?.clientId ?? "")}
            title={t("chat.terminal.restart")}
            aria-label={t("chat.terminal.restart")}
          >
            <RefreshCw size={14} aria-hidden />
          </button>
          <button
            type="button"
            onClick={onClose}
            title={t("chat.terminal.close")}
            aria-label={t("chat.terminal.close")}
          >
            <PanelBottomClose size={15} aria-hidden />
          </button>
        </div>
      </header>
      {activeSession ? (
        <TerminalPane
          key={activeSession.id}
          clientId={activeTab.clientId}
          visible={open}
          session={activeSession}
          settings={settings}
          startCursor={
            clearState[activeTab.clientId]?.cursor ?? activeSession.baseCursor
          }
          clearGeneration={clearState[activeTab.clientId]?.generation ?? 0}
          onSessionProgress={updateSessionProgress}
          onError={(message) => {
            if (!activeTab) return;
            setErrors((current) => {
              const next = { ...current };
              if (message) next[activeTab.clientId] = message;
              else delete next[activeTab.clientId];
              return next;
            });
          }}
        />
      ) : (
        <div className="terminal-dock-loading">
          {activeError ?? t("chat.terminal.tab.starting")}
        </div>
      )}
    </section>
  );
}
