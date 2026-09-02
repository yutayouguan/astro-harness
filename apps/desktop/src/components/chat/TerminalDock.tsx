import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XtermTerminal } from "@xterm/xterm";
import { ExternalLink, Square, Trash2, X } from "lucide-react";
import "@xterm/xterm/css/xterm.css";

import { useI18n } from "../../i18n/LocaleContext";

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
  projectId: string;
  projectName: string;
  projectRoot: string;
  onClose: () => void;
};

const HEIGHT_KEY_PREFIX = "astro.terminalDock.height.";

function initialHeight(projectId: string): number {
  const stored = Number(localStorage.getItem(`${HEIGHT_KEY_PREFIX}${projectId}`));
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
  projectId,
  projectName,
  projectRoot,
  onClose,
}: Props) {
  const { t } = useI18n();
  const hostRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XtermTerminal | null>(null);
  const [height, setHeight] = useState(() => initialHeight(projectId));
  const [session, setSession] = useState<TerminalSessionDto | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setHeight(initialHeight(projectId));
  }, [projectId]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host || !projectRoot) return;
    let disposed = false;
    let cursor = 0;
    let pendingInput = "";
    let writing = false;
    const sessionRef = { current: null as TerminalSessionDto | null };

    const terminal = new XtermTerminal({
      allowProposedApi: false,
      convertEol: false,
      cursorBlink: true,
      cursorStyle: "bar",
      fontFamily:
        '"SFMono-Regular", "SF Mono", Menlo, Monaco, Consolas, monospace',
      fontSize: 12.5,
      lineHeight: 1.28,
      scrollback: 5_000,
      theme: terminalTheme(),
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(host);
    xtermRef.current = terminal;
    fit.fit();

    const flushInput = async () => {
      if (writing || !pendingInput || !sessionRef.current) return;
      writing = true;
      try {
        while (pendingInput && sessionRef.current && !disposed) {
          const data = pendingInput;
          pendingInput = "";
          await invoke("terminal_write", {
            request: {
              id: sessionRef.current.id,
              data: Array.from(new TextEncoder().encode(data)),
            },
          });
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
    const resizeDisposable = terminal.onResize(({ cols, rows }) => {
      const current = sessionRef.current;
      if (!current) return;
      void invoke("terminal_resize", {
        request: { id: current.id, cols, rows },
      }).catch(() => undefined);
    });
    const observer = new ResizeObserver(() => {
      fit.fit();
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
          },
        });
        if (disposed) return;
        sessionRef.current = opened;
        setSession(opened);
        setError(null);
        cursor = opened.baseCursor;
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
            terminal.writeln(
              `\r\n[${t("chat.terminal.exited", { code: String(result.exitCode ?? "-") })}]`,
            );
            break;
          }
        }
      } catch (reason) {
        if (!disposed) {
          const message = String(reason);
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
      inputDisposable.dispose();
      resizeDisposable.dispose();
      terminal.dispose();
      xtermRef.current = null;
    };
  }, [projectRoot, t]);

  const beginResize = useCallback(
    (event: React.PointerEvent<HTMLButtonElement>) => {
      event.preventDefault();
      const startY = event.clientY;
      const startHeight = height;
      const move = (moveEvent: PointerEvent) => {
        const max = Math.min(720, Math.floor(window.innerHeight * 0.65));
        setHeight(Math.max(160, Math.min(max, startHeight + startY - moveEvent.clientY)));
      };
      const up = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up, { once: true });
    },
    [height],
  );

  useEffect(() => {
    localStorage.setItem(`${HEIGHT_KEY_PREFIX}${projectId}`, String(Math.round(height)));
  }, [height, projectId]);

  const openExternal = useCallback(() => {
    void invoke("terminal_open_external", { cwd: projectRoot }).catch((reason) =>
      setError(String(reason)),
    );
  }, [projectRoot]);

  const kill = useCallback(() => {
    if (!session?.running) return;
    void invoke("terminal_kill", { id: session.id }).catch((reason) =>
      setError(String(reason)),
    );
  }, [session]);

  return (
    <section
      className="terminal-dock"
      style={{ "--terminal-dock-height": `${height}px` } as React.CSSProperties}
      aria-label={t("chat.terminal.title")}
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
          <span className="terminal-dock-shared">{t("chat.terminal.shared")}</span>
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
            onClick={kill}
            disabled={!session?.running}
            title={t("chat.terminal.kill")}
            aria-label={t("chat.terminal.kill")}
          >
            <Square size={12} aria-hidden />
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
