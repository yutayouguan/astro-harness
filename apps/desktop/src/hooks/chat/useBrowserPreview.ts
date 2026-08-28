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

function loadStored(sessionId: string | null): BrowserPreview | null {
  if (!sessionId) return null;
  try {
    const value = parseRecord(localStorage.getItem(`${STORAGE_PREFIX}${sessionId}`) ?? undefined);
    if (!value) return null;
    const status = text(value.status) as BrowserPreviewStatus;
    return {
      sessionId,
      url: text(value.url),
      title: text(value.title),
      screenshotPath: text(value.screenshotPath) || null,
      status:
        status === "connected" || status === "connecting"
          ? "disconnected"
          : status || "disconnected",
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
      const name = call.name?.toLowerCase() ?? "";
      if (!name.startsWith("browser_")) return;
      const args = parseRecord(call.arguments_json);
      const result = parseRecord(call.result);
      const now = Date.now();

      if (call.phase === "started") {
        setDismissed(false);
        setPreview((current) => ({
          sessionId: sessionId ?? current?.sessionId ?? "pending",
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
        const status = text(result.status) as BrowserPreviewStatus;
        const actionRecord =
          result.action && typeof result.action === "object"
            ? (result.action as Record<string, unknown>)
            : null;
        setDismissed(status === "closed");
        setPreview((current) => ({
          sessionId: sessionId ?? current?.sessionId ?? "pending",
          url: text(result.url) || current?.url || "",
          title: text(result.title) || current?.title || "",
          screenshotPath:
            text(result.screenshot_path) || current?.screenshotPath || null,
          status: status || "connected",
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

  const dismiss = useCallback(() => setDismissed(true), []);
  const api = useMemo<BrowserPreviewApi>(() => ({ onToolCall }), [onToolCall]);

  return { preview: dismissed ? null : preview, api, dismiss };
}
