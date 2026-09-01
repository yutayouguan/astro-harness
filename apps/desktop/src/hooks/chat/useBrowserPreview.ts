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

function loadStored(sessionId: string | null): BrowserPreview | null {
  if (!sessionId) return null;
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
  const [preview, setPreview] = useState<BrowserPreview | null>(() =>
    loadStored(sessionId),
  );
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    setPreview(loadStored(sessionId));
    setDismissed(false);
  }, [sessionId]);

  useEffect(() => {
    if (!sessionId || !preview) return;
    try {
      localStorage.setItem(
        `${STORAGE_PREFIX}${sessionId}`,
        JSON.stringify(preview),
      );
    } catch {
      // Persistence is best-effort; live preview remains available.
    }
  }, [preview, sessionId]);

  const applyResult = useCallback(
    (raw: unknown, fallbackAction?: string) => {
      if (!sessionId) return;
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
          localStorage.removeItem(`${STORAGE_PREFIX}${sessionId}`);
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
        sessionId,
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
    [sessionId],
  );

  const onToolCall = useCallback(
    (call: {
      name?: string;
      arguments_json?: string;
      result?: string;
      phase?: string;
    }) => {
      if (!sessionId) return;
      const name = call.name?.toLowerCase() ?? "";
      if (!name.startsWith("browser_")) return;
      const args = parseRecord(call.arguments_json);
      const result = parseRecord(call.result);
      const now = Date.now();

      if (call.phase === "started") {
        setDismissed(false);
        setPreview((current) => ({
          sessionId,
          url: text(args?.url) || current?.url || "",
          title: current?.title || "",
          screenshotPath: current?.screenshotPath ?? null,
          status: name === "browser_close" ? "closed" : "connecting",
          action: name.replace(/^browser_/, ""),
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
    [applyResult, sessionId],
  );

  const control = useCallback(
    async (action: string, args: Record<string, unknown> = {}) => {
      if (!sessionId) throw new Error("需要先开始一个对话");
      setDismissed(false);
      setPreview((current) =>
        current ? { ...current, status: "connecting", action } : current,
      );
      try {
        const result = await invoke<Record<string, unknown>>(
          "browser_panel_control",
          { request: { sessionId, action, args } },
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
    [applyResult, sessionId],
  );

  const dismiss = useCallback(() => {
    setDismissed(true);
    setPreview(null);
    if (!sessionId) return;
    try {
      localStorage.removeItem(`${STORAGE_PREFIX}${sessionId}`);
    } catch {
      // Persistence is best-effort.
    }
  }, [sessionId]);
  const api = useMemo<BrowserPreviewApi>(() => ({ onToolCall }), [onToolCall]);

  return {
    preview: !dismissed && preview?.sessionId === sessionId ? preview : null,
    api,
    control,
    applyResult,
    dismiss,
  };
}
