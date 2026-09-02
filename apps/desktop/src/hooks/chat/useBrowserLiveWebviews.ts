import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type RefObject,
} from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { LogicalPosition, LogicalSize } from "@tauri-apps/api/dpi";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Webview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { BrowserPreview, BrowserPreviewTab } from "./useBrowserPreview";

const LIVE_WEBVIEW_PREFIX = "astro-browser-live-";
const LIVE_PAGE_EVENT = "browser-live-page-load";

type LiveWebviewStatus =
  | "unsupported"
  | "idle"
  | "creating"
  | "loading"
  | "ready"
  | "error";

type LivePageEvent = {
  label: string;
  url: string;
  status: "started" | "finished" | "blocked";
  error?: string;
};

type ManagedWebview = {
  webview: Webview;
  url: string;
  ready: boolean;
  visible: boolean;
  loading: boolean;
};

type LiveWebviewOptions = {
  preview: BrowserPreview | null;
  viewportRef: RefObject<HTMLDivElement | null>;
  onUrlChange: (url: string) => void;
  onNavigate: (url: string) => void | Promise<void>;
  onError: (message: string) => void;
};

function hashLabelPart(value: string): string {
  let hash = 0x811c9dc5;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(36);
}

export function browserLiveWebviewLabel(
  sessionId: string,
  tabId: string,
): string {
  return `${LIVE_WEBVIEW_PREFIX}${hashLabelPart(sessionId)}-${hashLabelPart(tabId)}`;
}

function canonicalUrl(raw: string): string {
  try {
    return new URL(raw).href;
  } catch {
    return raw.trim();
  }
}

function activeTab(preview: BrowserPreview | null): BrowserPreviewTab | null {
  if (!preview?.url) return null;
  return (
    preview.tabs.find((tab) => tab.id === preview.activeTabId) ??
    preview.tabs.find((tab) => tab.active) ?? {
      id: preview.activeTabId || "active",
      title: preview.title,
      url: preview.url,
      active: true,
    }
  );
}

function viewportBounds(viewport: HTMLDivElement) {
  const rect = viewport.getBoundingClientRect();
  return {
    x: Math.round(rect.left),
    y: Math.round(rect.top),
    width: Math.round(rect.width),
    height: Math.round(rect.height),
  };
}

export function useBrowserLiveWebviews({
  preview,
  viewportRef,
  onUrlChange,
  onNavigate,
  onError,
}: LiveWebviewOptions) {
  const [status, setStatus] = useState<LiveWebviewStatus>(() =>
    isTauri() ? "idle" : "unsupported",
  );
  const previewRef = useRef(preview);
  const callbacksRef = useRef({ onUrlChange, onNavigate, onError });
  const scheduleRef = useRef<() => void>(() => undefined);

  previewRef.current = preview;
  callbacksRef.current = { onUrlChange, onNavigate, onError };

  useEffect(() => {
    if (!isTauri()) {
      setStatus("unsupported");
      return;
    }

    let disposed = false;
    let animationFrame = 0;
    let unlisten: UnlistenFn | null = null;
    let observer: ResizeObserver | null = null;
    let lastReloadKey = "";
    let lastNativeSync = "";
    const managed = new Map<string, ManagedWebview>();
    const pendingCreates = new Map<string, Promise<ManagedWebview>>();

    const reportError = (cause: unknown) => {
      if (disposed) return;
      const message = cause instanceof Error ? cause.message : String(cause);
      for (const entry of managed.values()) {
        if (!entry.ready || !entry.visible) continue;
        entry.visible = false;
        void entry.webview.hide().catch(() => undefined);
      }
      setStatus("error");
      callbacksRef.current.onError(message);
    };

    const updateBounds = async (entry: ManagedWebview) => {
      const viewport = viewportRef.current;
      if (!viewport || !entry.ready) return;
      const bounds = viewportBounds(viewport);
      if (bounds.width < 1 || bounds.height < 1) return;
      await Promise.all([
        entry.webview.setPosition(new LogicalPosition(bounds.x, bounds.y)),
        entry.webview.setSize(new LogicalSize(bounds.width, bounds.height)),
      ]);
    };

    const createWebview = async (
      label: string,
      url: string,
    ): Promise<ManagedWebview> => {
      const existing = await Webview.getByLabel(label);
      if (existing) {
        const entry = {
          webview: existing,
          url: "",
          ready: true,
          visible: false,
          loading: false,
        };
        managed.set(label, entry);
        setStatus("ready");
        return entry;
      }

      const viewport = viewportRef.current;
      if (!viewport) throw new Error("浏览器显示区尚未就绪");
      const bounds = viewportBounds(viewport);
      if (bounds.width < 1 || bounds.height < 1) {
        throw new Error("浏览器显示区尺寸无效");
      }

      setStatus("creating");
      const webview = new Webview(getCurrentWindow(), label, {
        url,
        ...bounds,
        focus: false,
        acceptFirstMouse: true,
        zoomHotkeysEnabled: true,
      });
      const entry = {
        webview,
        url,
        ready: false,
        visible: true,
        loading: true,
      };
      managed.set(label, entry);

      void webview.once("tauri://created", () => {
        if (disposed) {
          void webview.close();
          return;
        }
        entry.ready = true;
        setStatus("loading");
        scheduleRef.current();
      });
      void webview.once<string>("tauri://error", (event) => {
        managed.delete(label);
        reportError(event.payload || "创建实时浏览器 WebView 失败");
      });
      return entry;
    };

    const ensureWebview = (label: string, url: string) => {
      const current = managed.get(label);
      if (current) return Promise.resolve(current);
      const pending = pendingCreates.get(label);
      if (pending) return pending;
      const creation = createWebview(label, url).finally(() => {
        pendingCreates.delete(label);
      });
      pendingCreates.set(label, creation);
      return creation;
    };

    const sync = async () => {
      if (disposed) return;
      const current = previewRef.current;
      const tab = activeTab(current);
      const viewport = viewportRef.current;
      if (!current || !tab || !viewport) {
        for (const entry of managed.values()) {
          if (entry.ready && entry.visible) {
            entry.visible = false;
            void entry.webview.hide().catch(reportError);
          }
        }
        setStatus("idle");
        return;
      }

      const activeLabel = browserLiveWebviewLabel(current.sessionId, tab.id);
      const liveLabels = new Set(
        (current.tabs.length ? current.tabs : [tab]).map((item) =>
          browserLiveWebviewLabel(current.sessionId, item.id),
        ),
      );
      for (const [label, entry] of managed) {
        if (liveLabels.has(label)) continue;
        managed.delete(label);
        void entry.webview.close().catch(() => undefined);
      }

      let entry = managed.get(activeLabel);
      if (!entry) entry = await ensureWebview(activeLabel, tab.url || current.url);

      for (const [label, candidate] of managed) {
        if (!candidate.ready) continue;
        const shouldShow = label === activeLabel;
        if (candidate.visible !== shouldShow) {
          candidate.visible = shouldShow;
          void (shouldShow ? candidate.webview.show() : candidate.webview.hide()).catch(
            reportError,
          );
        }
      }

      if (!entry.ready) return;
      await updateBounds(entry);

      const desiredUrl = tab.url || current.url;
      if (desiredUrl && canonicalUrl(desiredUrl) !== canonicalUrl(entry.url)) {
        const previousUrl = entry.url;
        entry.url = desiredUrl;
        entry.loading = true;
        setStatus("loading");
        try {
          await invoke("browser_live_webview_control", {
            request: { label: activeLabel, action: "navigate", url: desiredUrl },
          });
        } catch (cause) {
          entry.url = previousUrl;
          reportError(cause);
        }
      }

      if (current.status === "connected" && current.action === "reload") {
        const reloadKey = `${activeLabel}:${current.updatedAt}`;
        if (reloadKey !== lastReloadKey) {
          lastReloadKey = reloadKey;
          entry.loading = true;
          setStatus("loading");
          void invoke("browser_live_webview_control", {
            request: { label: activeLabel, action: "reload" },
          }).catch(reportError);
        }
      }
      setStatus(entry.loading ? "loading" : "ready");
    };

    const schedule = () => {
      if (disposed) return;
      window.cancelAnimationFrame(animationFrame);
      animationFrame = window.requestAnimationFrame(() => {
        void sync().catch(reportError);
      });
    };
    scheduleRef.current = schedule;

    const viewport = viewportRef.current;
    if (viewport && typeof ResizeObserver !== "undefined") {
      observer = new ResizeObserver(schedule);
      observer.observe(viewport);
    }
    window.addEventListener("resize", schedule);
    window.addEventListener("scroll", schedule, true);

    void listen<LivePageEvent>(LIVE_PAGE_EVENT, ({ payload }) => {
      if (disposed) return;
      const entry = managed.get(payload.label);
      if (!entry) return;
      if (payload.status === "blocked") {
        entry.url = "";
        entry.loading = false;
        entry.visible = false;
        void entry.webview.hide().catch(() => undefined);
        reportError(payload.error || "该网址已被浏览器权限设置拦截");
        return;
      }

      entry.url = payload.url;
      entry.loading = payload.status === "started";
      const current = previewRef.current;
      const tab = activeTab(current);
      if (
        !current ||
        !tab ||
        payload.label !== browserLiveWebviewLabel(current.sessionId, tab.id)
      ) {
        return;
      }

      callbacksRef.current.onUrlChange(payload.url);
      setStatus(payload.status === "started" ? "loading" : "ready");
      if (payload.status !== "finished") return;
      const nativeUrl = canonicalUrl(payload.url);
      if (!nativeUrl || nativeUrl === canonicalUrl(current.url)) return;
      const syncKey = `${payload.label}:${nativeUrl}`;
      if (syncKey === lastNativeSync) return;
      lastNativeSync = syncKey;
      void Promise.resolve(callbacksRef.current.onNavigate(payload.url)).catch(
        reportError,
      );
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(reportError);

    schedule();
    return () => {
      disposed = true;
      scheduleRef.current = () => undefined;
      window.cancelAnimationFrame(animationFrame);
      window.removeEventListener("resize", schedule);
      window.removeEventListener("scroll", schedule, true);
      observer?.disconnect();
      unlisten?.();
      for (const entry of managed.values()) {
        void entry.webview.close().catch(() => undefined);
      }
      managed.clear();
      pendingCreates.clear();
    };
  }, [viewportRef]);

  useLayoutEffect(() => {
    scheduleRef.current();
  }, [
    preview?.sessionId,
    preview?.activeTabId,
    preview?.url,
    preview?.status,
    preview?.action,
    preview?.updatedAt,
    preview?.tabs,
  ]);

  return {
    status,
    isLive: status !== "unsupported" && status !== "error",
    isLoading: status === "creating" || status === "loading",
  };
}
