import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XtermTerminal } from "@xterm/xterm";
import { ExternalLink, RefreshCw, Trash2, X } from "lucide-react";
import "@xterm/xterm/css/xterm.css";

import { useI18n } from "../../i18n/LocaleContext";
import {
  readTerminalSettings,
  subscribeTerminalSettings,
} from "../../lib/terminal/terminalSettings";

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
  open: boolean;
  projectId: string;
  projectName: string;
  projectRoot: string;
  onClose: () => void;
};

const HEIGHT_KEY_PREFIX = "astro.terminalDock.height.";
const WRITE_CHUNK_BYTES = 32 * 1024;
const SCROLLBAR_WIDTH = 4;

function initialHeight(projectId: string): number {
  const stored = Number(
    localStorage.getItem(`${HEIGHT_KEY_PREFIX}${projectId}`),
  );
  return Number.isFinite(stored) && stored >= 160 ? stored : 260;
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

export default function TerminalDock({
  open,
  projectId,
  projectName,
  projectRoot,
  onClose,
}: Props) {
  const { t } = useI18n();
  const hostRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XtermTerminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const restartAfterExitRef = useRef(false);
  const resizeDragCleanupRef = useRef<(() => void) | null>(null);
  const [height, setHeight] = useState(() => initialHeight(projectId));
  const [session, setSession] = useState<TerminalSessionDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [restarting, setRestarting] = useState(false);
  const [restartGeneration, setRestartGeneration] = useState(0);
  const [settings, setSettings] = useState(readTerminalSettings);

  useEffect(() => subscribeTerminalSettings(setSettings), []);

  useEffect(() => {
    setHeight(initialHeight(projectId));
  }, [projectId]);

  useEffect(() => {
    const host = hostRef.current;
    if (!open || !host || !projectRoot) return;
    setSession(null);
    setError(null);
    restartAfterExitRef.current = false;
    let disposed = false;
    let cursor = 0;
    let pendingInput = "";
    let writing = false;
    const sessionRef = { current: null as TerminalSessionDto | null };

    const terminal = new XtermTerminal({
      allowProposedApi: false,
      allowTransparency: true,
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
    fit.fit();

    const flushInput = async () => {
      if (writing || !pendingInput || !sessionRef.current) return;
      writing = true;
      try {
        while (pendingInput && sessionRef.current && !disposed) {
          const data = pendingInput;
          pendingInput = "";
          const encoded = new TextEncoder().encode(data);
          for (
            let offset = 0;
            offset < encoded.length;
            offset += WRITE_CHUNK_BYTES
          ) {
            await invoke("terminal_write", {
              request: {
                id: sessionRef.current.id,
                data: Array.from(
                  encoded.subarray(offset, offset + WRITE_CHUNK_BYTES),
                ),
              },
            });
          }
        }
      } catch (reason) {
        if (!disposed) setError(String(reason));
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
        const current = sessionRef.current;
        if (!current || disposed) return;
        void invoke("terminal_resize", {
          request: { id: current.id, cols, rows },
        }).catch(() => undefined);
      }, 60);
    });
    let fitFrame: number | null = null;
    const observer = new ResizeObserver(() => {
      if (fitFrame !== null) return;
      fitFrame = window.requestAnimationFrame(() => {
        fitFrame = null;
        if (!disposed) fit.fit();
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

    const run = async () => {
      try {
        const opened = await invoke<TerminalSessionDto>("terminal_open", {
          request: {
            scope: projectRoot,
            cwd: projectRoot,
            cols: terminal.cols,
            rows: terminal.rows,
            executionMode: settings.executionMode,
          },
        });
        if (disposed) return;
        sessionRef.current = opened;
        setSession(opened);
        setError(null);
        setRestarting(false);
        cursor = opened.baseCursor;
        void flushInput();
        terminal.focus();

        while (!disposed) {
          const result = await invoke<TerminalReadResultDto>("terminal_read", {
            request: {
              id: opened.id,
              cursor,
              maxBytes: 64 * 1024,
              waitMs: 5_000,
            },
          });
          if (disposed) return;
          cursor = result.nextCursor;
          if (result.dropped) {
            terminal.writeln(`\r\n[${t("chat.terminal.outputTruncated")}]`);
          }
          if (result.data.length > 0) {
            terminal.write(Uint8Array.from(result.data));
          }
          setSession((current) =>
            current?.id === result.id
              ? {
                  ...current,
                  running: result.running,
                  exitCode: result.exitCode,
                  endCursor: result.nextCursor,
                }
              : current,
          );
          if (!result.running) {
            if (restartAfterExitRef.current) {
              restartAfterExitRef.current = false;
              setRestartGeneration((generation) => generation + 1);
            } else {
              terminal.writeln(
                `\r\n[${t("chat.terminal.exited", { code: String(result.exitCode ?? "-") })}]`,
              );
            }
            break;
          }
        }
      } catch (reason) {
        if (!disposed) {
          const message = String(reason);
          setRestarting(false);
          setError(message);
          terminal.writeln(`\r\n[${message}]`);
        }
      }
    };

    void run();
    return () => {
      disposed = true;
      sessionRef.current = null;
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
  }, [open, projectRoot, restartGeneration, settings.executionMode, t]);

  useEffect(() => {
    const terminal = xtermRef.current;
    if (!terminal) return;
    terminal.options.fontFamily = settings.fontFamily;
    terminal.options.fontSize = settings.fontSize;
    terminal.options.lineHeight = settings.lineHeight;
    terminal.options.scrollback = settings.scrollback;
    terminal.options.cursorStyle = settings.cursorStyle;
    terminal.options.cursorBlink = settings.cursorBlink;
    window.requestAnimationFrame(() => fitRef.current?.fit());
  }, [settings]);

  const beginResize = useCallback(
    (event: React.PointerEvent<HTMLButtonElement>) => {
      event.preventDefault();
      const startY = event.clientY;
      const startHeight = height;
      resizeDragCleanupRef.current?.();
      const move = (moveEvent: PointerEvent) => {
        const max = Math.min(720, Math.floor(window.innerHeight * 0.65));
        setHeight(
          Math.max(
            160,
            Math.min(max, startHeight + startY - moveEvent.clientY),
          ),
        );
      };
      const cleanup = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", cleanup);
        resizeDragCleanupRef.current = null;
      };
      resizeDragCleanupRef.current = cleanup;
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", cleanup, { once: true });
    },
    [height],
  );

  useEffect(
    () => () => {
      resizeDragCleanupRef.current?.();
    },
    [],
  );

  useEffect(() => {
    localStorage.setItem(
      `${HEIGHT_KEY_PREFIX}${projectId}`,
      String(Math.round(height)),
    );
  }, [height, projectId]);

  const openExternal = useCallback(() => {
    void invoke("terminal_open_external", { cwd: projectRoot }).catch(
      (reason) => setError(String(reason)),
    );
  }, [projectRoot]);

  const restart = useCallback(() => {
    if (restarting) return;
    setError(null);
    setRestarting(true);
    if (!session?.running) {
      setRestartGeneration((generation) => generation + 1);
      return;
    }
    restartAfterExitRef.current = true;
    void invoke("terminal_kill", { id: session.id }).catch((reason) => {
      restartAfterExitRef.current = false;
      setRestarting(false);
      setError(String(reason));
    });
  }, [restarting, session]);

  return (
    <section
      className={`terminal-dock${open ? " is-open" : ""}`}
      style={{ "--terminal-dock-height": `${height}px` } as React.CSSProperties}
      aria-label={t("chat.terminal.title")}
      aria-hidden={!open}
    >
      <button
        type="button"
        className="terminal-dock-resizer"
        onPointerDown={beginResize}
        aria-label={t("chat.terminal.resize")}
      />
      <header className="terminal-dock-header">
        <div className="terminal-dock-identity">
          <span
            className={`terminal-dock-status ${session?.running ? "is-running" : ""}`}
            aria-hidden
          />
          <strong>{t("chat.terminal.title")}</strong>
          <span>{projectName}</span>
          <span className="terminal-dock-shared">
            {t(
              settings.executionMode === "system"
                ? "chat.terminal.mode.system"
                : "chat.terminal.mode.project",
            )}
          </span>
        </div>
        <div className="terminal-dock-actions">
          {error ? <span className="terminal-dock-error">{error}</span> : null}
          <button
            type="button"
            onClick={openExternal}
            title={t("chat.terminal.external")}
            aria-label={t("chat.terminal.external")}
          >
            <ExternalLink size={14} aria-hidden />
          </button>
          <button
            type="button"
            onClick={() => {
              xtermRef.current?.clear();
              setError(null);
            }}
            title={t("chat.terminal.clear")}
            aria-label={t("chat.terminal.clear")}
            className="terminal-clear-button"
          >
            <Trash2 size={14} aria-hidden />
          </button>
          <button
            type="button"
            onClick={restart}
            disabled={restarting || (!session && !error)}
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
            <X size={15} aria-hidden />
          </button>
        </div>
      </header>
      <div ref={hostRef} className="terminal-dock-screen" />
    </section>
  );
}
