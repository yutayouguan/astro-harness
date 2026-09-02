import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export type BrowserPreviewStatus =
  "connecting" | "connected" | "disconnected" | "closed" | "error";

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
  const [preview, setPreview] = useState<BrowserPreview | null>(() =>
    loadStored(browserSessionId),
  );
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    setPreview(loadStored(browserSessionId));
    setDismissed(false);
  }, [browserSessionId]);

  useEffect(() => {
    if (!preview) return;
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
      setPreview((current) => ({
        sessionId: browserSessionId,
        url: text(result.url) || current?.url || "",
        title: text(result.title) || current?.title || "",
        screenshotPath:
          text(result.screenshot_path ?? result.screenshotPath) ||
          current?.screenshotPath ||
          null,
        status: nextStatus,
        action:
          text(actionRecord?.kind) || fallbackAction || current?.action || null,
        updatedAt: Date.now(),
        activeTabId:
          text(result.active_tab_id ?? result.activeTabId) ||
          current?.activeTabId ||
          null,
        tabs: Array.isArray(result.tabs)
          ? browserTabs(result.tabs)
          : current?.tabs || [],
        downloads: Array.isArray(result.downloads)
          ? browserDownloads(result.downloads)
          : current?.downloads || [],
      }));
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
      const name = call.name?.toLowerCase() ?? "";
      if (!name.startsWith("browser_")) return;
      const args = parseRecord(call.arguments_json);
      const result = parseRecord(call.result);
      const now = Date.now();

      if (call.phase === "started") {
        const action = name.replace(/^browser_/, "");
        setDismissed(false);
        setPreview((current) => ({
          sessionId: browserSessionId,
          url: text(args?.url) || current?.url || "",
          title: current?.title || "",
          screenshotPath: current?.screenshotPath ?? null,
          status:
            name === "browser_close"
              ? "closed"
              : isNavigationAction(name)
                ? "connecting"
                : current?.status || "connected",
          action,
          updatedAt: now,
          activeTabId: current?.activeTabId ?? null,
          tabs: current?.tabs ?? [],
          downloads: current?.downloads ?? [],
        }));
        return;
      }

      if (result?.astro_browser === true) {
        applyResult(result, name.replace(/^browser_/, ""));
      } else if (call.result && call.phase === "completed") {
        setPreview((current) =>
          current
            ? {
                ...current,
                status: "error",
                action: name.replace(/^browser_/, ""),
                updatedAt: now,
              }
            : current,
        );
      }
    },
    [applyResult, browserSessionId],
  );

  const control = useCallback(
    async (action: string, args: Record<string, unknown> = {}) => {
      setDismissed(false);
      setPreview((current) =>
        current
          ? {
              ...current,
              status: isNavigationAction(action)
                ? "connecting"
                : current.status,
              action,
            }
          : current,
      );
      try {
        const result = await invoke<Record<string, unknown>>(
          "browser_panel_control",
          { request: { sessionId: browserSessionId, action, args } },
        );
        applyResult(result, action);
        return result;
      } catch (error) {
        setPreview((current) =>
          current ? { ...current, status: "error", action } : current,
        );
        throw error;
      }
    },
    [applyResult, browserSessionId],
  );

  const dismiss = useCallback(() => {
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
