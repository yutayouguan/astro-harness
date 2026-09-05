import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { normalizeBrowserFaviconUrl } from "../../lib/browser/browserFavicon";
import { nextBrowserPreviewRevision } from "../../lib/browser/browserPreviewState";

export type BrowserPreviewStatus =
  | "connecting"
  | "connected"
  | "disconnected"
  | "closed"
  | "error";

export type BrowserPreview = {
  sessionId: string;
  url: string;
  title: string;
  screenshotPath: string | null;
  status: BrowserPreviewStatus;
  action: string | null;
  updatedAt: number;
  activeTabId: string | null;
  tabs: BrowserPreviewTab[];
  downloads: BrowserDownload[];
};

export type BrowserPreviewTab = {
  id: string;
  title: string;
  url: string;
  faviconUrl: string | null;
  active: boolean;
};

export type BrowserDownload = {
  name: string;
  path: string;
  size: number;
  status: "downloading" | "complete";
  updatedAt: number;
};

export type BrowserPreviewApi = {
  onToolCall: (call: {
    name?: string;
    arguments_json?: string;
    result?: string;
    phase?: string;
  }) => void;
};

const STORAGE_PREFIX = "astro.browserPreview.";
export const DESKTOP_BROWSER_SESSION_ID = "desktop-browser-default";
const NAVIGATION_ACTIONS = new Set([
  "open",
  "new_tab",
  "switch_tab",
  "back",
  "forward",
  "reload",
  "browser_open",
  "browser_tab_open",
  "browser_tab_switch",
  "browser_back",
  "browser_forward",
  "browser_reload",
]);

function isNavigationAction(action: string): boolean {
  return NAVIGATION_ACTIONS.has(action);
}

function parseRecord(raw: string | undefined): Record<string, unknown> | null {
  if (!raw?.trim()) return null;
  try {
    const value = JSON.parse(raw);
    return value && typeof value === "object" && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function browserTabs(value: unknown): BrowserPreviewTab[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((item) => {
    if (!item || typeof item !== "object") return [];
    const row = item as Record<string, unknown>;
    const id = text(row.id);
    if (!id) return [];
    return [
      {
        id,
        title: text(row.title),
        url: text(row.url),
        faviconUrl: normalizeBrowserFaviconUrl(
          row.favicon_url ?? row.faviconUrl,
        ),
        active: row.active === true,
      },
    ];
  });
}

function browserDownloads(value: unknown): BrowserDownload[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((item) => {
    if (!item || typeof item !== "object") return [];
    const row = item as Record<string, unknown>;
    const path = text(row.path);
    if (!path) return [];
    return [
      {
        name: text(row.name) || path.split(/[\\/]/).pop() || path,
        path,
        size: Number(row.size) || 0,
        status: row.status === "downloading" ? "downloading" : "complete",
        updatedAt: Number(row.updated_at ?? row.updatedAt) || 0,
      },
    ];
  });
}

function status(
  value: unknown,
  fallback: BrowserPreviewStatus,
): BrowserPreviewStatus {
  return value === "connecting" ||
    value === "connected" ||
    value === "disconnected" ||
    value === "closed" ||
    value === "error"
    ? value
    : fallback;
}

function loadStored(sessionId: string): BrowserPreview | null {
  try {
    const value = parseRecord(
      localStorage.getItem(`${STORAGE_PREFIX}${sessionId}`) ?? undefined,
    );
    if (!value) return null;
    const storedStatus = status(value.status, "disconnected");
    if (storedStatus === "closed") return null;
    return {
      sessionId,
      url: text(value.url),
      title: text(value.title),
      screenshotPath: text(value.screenshotPath) || null,
      status:
        storedStatus === "connected" || storedStatus === "connecting"
          ? "disconnected"
          : storedStatus,
      action: text(value.action) || null,
      updatedAt: Number(value.updatedAt) || Date.now(),
      activeTabId: text(value.activeTabId) || null,
      tabs: browserTabs(value.tabs),
      downloads: browserDownloads(value.downloads),
    };
  } catch {
    return null;
  }
}

export function useBrowserPreview(sessionId: string | null) {
  const browserSessionId = sessionId ?? DESKTOP_BROWSER_SESSION_ID;
  const browserSessionIdRef = useRef(browserSessionId);
  const controlGenerationRef = useRef(0);
  const controlQueueRef = useRef<Promise<void>>(Promise.resolve());
  const [preview, setPreview] = useState<BrowserPreview | null>(() =>
    loadStored(browserSessionId),
  );
  const [dismissed, setDismissed] = useState(false);

  browserSessionIdRef.current = browserSessionId;

  useEffect(() => {
    controlGenerationRef.current += 1;
    controlQueueRef.current = Promise.resolve();
    setPreview(loadStored(browserSessionId));
    setDismissed(false);
  }, [browserSessionId]);

  useEffect(() => {
    if (!preview || preview.sessionId !== browserSessionId) return;
    try {
      localStorage.setItem(
        `${STORAGE_PREFIX}${browserSessionId}`,
        JSON.stringify(preview),
      );
    } catch {
      // Persistence is best-effort; live preview remains available.
    }
  }, [browserSessionId, preview]);

  const applyResult = useCallback(
    (raw: unknown, fallbackAction?: string) => {
      if (browserSessionIdRef.current !== browserSessionId) return;
      const result =
        typeof raw === "string"
          ? parseRecord(raw)
          : raw && typeof raw === "object"
            ? (raw as Record<string, unknown>)
            : null;
      if (result?.astro_browser !== true) return;
      const nextStatus = status(result.status, "connected");
      if (nextStatus === "closed") {
        setDismissed(true);
        setPreview(null);
        try {
          localStorage.removeItem(`${STORAGE_PREFIX}${browserSessionId}`);
        } catch {
          // Persistence is best-effort.
        }
        return;
      }
      const actionRecord =
        result.action && typeof result.action === "object"
          ? (result.action as Record<string, unknown>)
          : null;
      setDismissed(false);
      setPreview((current) => {
        const previous =
          current?.sessionId === browserSessionId ? current : null;
        return {
          sessionId: browserSessionId,
          url: text(result.url) || previous?.url || "",
          title: text(result.title) || previous?.title || "",
          screenshotPath:
            text(result.screenshot_path ?? result.screenshotPath) ||
            previous?.screenshotPath ||
            null,
          status: nextStatus,
          action:
            text(actionRecord?.kind) ||
            fallbackAction ||
            previous?.action ||
            null,
          updatedAt: nextBrowserPreviewRevision(previous?.updatedAt),
          activeTabId:
            text(result.active_tab_id ?? result.activeTabId) ||
            previous?.activeTabId ||
            null,
          tabs: Array.isArray(result.tabs)
            ? browserTabs(result.tabs)
            : previous?.tabs || [],
          downloads: Array.isArray(result.downloads)
            ? browserDownloads(result.downloads)
            : previous?.downloads || [],
        };
      });
    },
    [browserSessionId],
  );

  const onToolCall = useCallback(
    (call: {
      name?: string;
      arguments_json?: string;
      result?: string;
      phase?: string;
    }) => {
      if (browserSessionIdRef.current !== browserSessionId) return;
      const name = call.name?.toLowerCase() ?? "";
      if (!name.startsWith("browser_")) return;
      const args = parseRecord(call.arguments_json);
      const result = parseRecord(call.result);
      if (call.phase === "started") {
        const action = name.replace(/^browser_/, "");
        setDismissed(false);
        setPreview((current) => {
          const previous =
            current?.sessionId === browserSessionId ? current : null;
          return {
            sessionId: browserSessionId,
            url: text(args?.url) || previous?.url || "",
            title: previous?.title || "",
            screenshotPath: previous?.screenshotPath ?? null,
            status:
              name === "browser_close"
                ? "closed"
                : isNavigationAction(name)
                  ? "connecting"
                  : previous?.status || "connected",
            action,
            updatedAt: nextBrowserPreviewRevision(previous?.updatedAt),
            activeTabId: previous?.activeTabId ?? null,
            tabs: previous?.tabs ?? [],
            downloads: previous?.downloads ?? [],
          };
        });
        return;
      }

      if (result?.astro_browser === true) {
        applyResult(result, name.replace(/^browser_/, ""));
      } else if (call.result && call.phase === "completed") {
        setPreview((current) =>
          current?.sessionId === browserSessionId
            ? {
                ...current,
                status: "error",
                action: name.replace(/^browser_/, ""),
                updatedAt: nextBrowserPreviewRevision(current.updatedAt),
              }
            : current,
        );
      }
    },
    [applyResult, browserSessionId],
  );

  const control = useCallback(
    async (action: string, args: Record<string, unknown> = {}) => {
      // Operations already complete in controlQueue order. The generation only
      // invalidates work from a previous browser session or a dismissed preview;
      // incrementing it per action would discard a successful navigation when a
      // queued resize starts before that navigation result is applied.
      const generation = controlGenerationRef.current;
      setDismissed(false);
      setPreview((current) =>
        current?.sessionId === browserSessionId
          ? {
              ...current,
              status: isNavigationAction(action)
                ? "connecting"
                : current.status,
              action,
              updatedAt: nextBrowserPreviewRevision(current.updatedAt),
            }
          : current,
      );
      const operation = controlQueueRef.current
        .catch(() => undefined)
        .then(() =>
          invoke<Record<string, unknown>>("browser_panel_control", {
            request: { sessionId: browserSessionId, action, args },
          }),
        );
      controlQueueRef.current = operation.then(
        () => undefined,
        () => undefined,
      );
      try {
        const result = await operation;
        if (
          browserSessionIdRef.current === browserSessionId &&
          controlGenerationRef.current === generation
        ) {
          applyResult(result, action);
        }
        return result;
      } catch (error) {
        if (
          browserSessionIdRef.current === browserSessionId &&
          controlGenerationRef.current === generation
        ) {
          setPreview((current) =>
            current?.sessionId === browserSessionId
              ? {
                  ...current,
                  status: "error",
                  action,
                  updatedAt: nextBrowserPreviewRevision(current.updatedAt),
                }
              : current,
          );
        }
        throw error;
      }
    },
    [applyResult, browserSessionId],
  );

  const dismiss = useCallback(() => {
    controlGenerationRef.current += 1;
    setDismissed(true);
    setPreview(null);
    try {
      localStorage.removeItem(`${STORAGE_PREFIX}${browserSessionId}`);
    } catch {
      // Persistence is best-effort.
    }
  }, [browserSessionId]);
  const api = useMemo<BrowserPreviewApi>(() => ({ onToolCall }), [onToolCall]);

  return {
    preview:
      !dismissed && preview?.sessionId === browserSessionId ? preview : null,
    api,
    control,
    applyResult,
    dismiss,
  };
}
