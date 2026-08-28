import { useCallback, useEffect, useMemo, useState } from "react";

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

function status(value: unknown, fallback: BrowserPreviewStatus): BrowserPreviewStatus {
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
    const value = parseRecord(localStorage.getItem(`${STORAGE_PREFIX}${sessionId}`) ?? undefined);
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
    };
  } catch {
    return null;
  }
}

export function useBrowserPreview(sessionId: string | null) {
  const [preview, setPreview] = useState<BrowserPreview | null>(() => loadStored(sessionId));
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    setPreview(loadStored(sessionId));
    setDismissed(false);
  }, [sessionId]);

  useEffect(() => {
    if (!sessionId || !preview) return;
    try {
      localStorage.setItem(`${STORAGE_PREFIX}${sessionId}`, JSON.stringify(preview));
    } catch {
      // Persistence is best-effort; live preview remains available.
    }
  }, [preview, sessionId]);

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
        }));
        return;
      }

      if (result?.astro_browser === true) {
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
            text(result.screenshot_path) || current?.screenshotPath || null,
          status: nextStatus,
          action: text(actionRecord?.kind) || name.replace(/^browser_/, ""),
          updatedAt: now,
        }));
      } else if (call.result && call.phase === "completed") {
        setPreview((current) =>
          current
            ? { ...current, status: "error", action: name.replace(/^browser_/, ""), updatedAt: now }
            : current,
        );
      }
    },
    [sessionId],
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
    dismiss,
  };
}
