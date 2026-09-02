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
import {
  LIVE_BROWSER_WEBVIEW_PREFIX,
  browserLiveWebviewLabel,
  browserWebviewBoundsKey,
  canonicalBrowserUrl,
  createBrowserLiveSurfaceId,
  retireBrowserLiveWebview,
  resolveLiveDesiredUrl,
} from "../../lib/browser/liveWebview";
import type { BrowserPreview, BrowserPreviewTab } from "./useBrowserPreview";

const LIVE_PAGE_EVENT = "browser-live-page-load";
const LOAD_STATUS_TIMEOUT_MS = 15_000;
let staleWebviewCleanup: Promise<void> | null = null;

/** Remove native browser surfaces left behind by a renderer reload or a close race. */
export function cleanupStaleBrowserLiveWebviews(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  if (staleWebviewCleanup) return staleWebviewCleanup;

  staleWebviewCleanup = (async () => {
    try {
      const currentWindowLabel = getCurrentWindow().label;
      const webviews = await Webview.getAll();
      await Promise.all(
        webviews
          .filter(
            (webview) =>
              webview.label.startsWith(LIVE_BROWSER_WEBVIEW_PREFIX) &&
              webview.window.label === currentWindowLabel,
          )
          .map(retireBrowserLiveWebview),
      );
    } catch {
      // The app may be running outside Tauri or shutting down already.
    } finally {
      staleWebviewCleanup = null;
    }
  })();

  return staleWebviewCleanup;
}

type LiveWebviewStatus =
  "unsupported" | "idle" | "creating" | "loading" | "ready" | "error";

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
  loadingTimer: number | null;
  boundsKey: string;
  failedUrl: string | null;
  failedAt: number | null;
};

type LiveWebviewOptions = {
  active?: boolean;
  occluded?: boolean;
  preview: BrowserPreview | null;
  viewportRef: RefObject<HTMLDivElement | null>;
  onUrlChange: (url: string) => void;
  onNavigate: (url: string) => void | Promise<void>;
  onError: (message: string | null) => void;
};

function activeTab(preview: BrowserPreview | null): BrowserPreviewTab | null {
  if (!preview?.url) return null;
  return (
    preview.tabs.find((tab) => tab.id === preview.activeTabId) ??
    preview.tabs.find((tab) => tab.active) ?? {
      id: preview.activeTabId || "active",
      title: preview.title,
      url: preview.url,
      faviconUrl: null,
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
  active = true,
  occluded = false,
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
  const occludedRef = useRef(occluded);
  const callbacksRef = useRef({ onUrlChange, onNavigate, onError });
  const scheduleRef = useRef<() => void>(() => undefined);

  previewRef.current = preview;
  occludedRef.current = occluded;
  callbacksRef.current = { onUrlChange, onNavigate, onError };

  useEffect(() => {
    if (!active) {
      setStatus(isTauri() ? "idle" : "unsupported");
      return;
    }
    if (!isTauri()) {
      setStatus("unsupported");
      return;
    }

    let disposed = false;
    let listenerReady = false;
    let animationFrame = 0;
    let unlisten: UnlistenFn | null = null;
    let observer: ResizeObserver | null = null;
    let lastReloadKey = "";
    let lastNativeSync = "";
    let nativeSyncQueue = Promise.resolve();
    let syncRunning = false;
    let syncRequested = false;
    const surfaceId = createBrowserLiveSurfaceId();
    const managed = new Map<string, ManagedWebview>();
    const pendingCreates = new Map<string, Promise<ManagedWebview>>();
    const pendingNativeUrls = new Map<string, string>();
    const labelFor = (sessionId: string, tabId: string) =>
      browserLiveWebviewLabel(sessionId, tabId, surfaceId);

    const setEntryLoading = (
      entry: ManagedWebview,
      label: string,
      loading: boolean,
    ) => {
      if (entry.loadingTimer !== null) {
        window.clearTimeout(entry.loadingTimer);
        entry.loadingTimer = null;
      }
      entry.loading = loading;
      if (!loading) return;
      entry.loadingTimer = window.setTimeout(() => {
        entry.loadingTimer = null;
        entry.loading = false;
        const current = previewRef.current;
        const tab = activeTab(current);
        if (
          current &&
          tab &&
          label === labelFor(current.sessionId, tab.id) &&
          !entry.failedUrl
        ) {
          setStatus("ready");
        }
      }, LOAD_STATUS_TIMEOUT_MS);
    };

    const reportError = (cause: unknown) => {
      if (disposed) return;
      const message = cause instanceof Error ? cause.message : String(cause);
      for (const entry of managed.values()) {
        if (entry.loadingTimer !== null) {
          window.clearTimeout(entry.loadingTimer);
          entry.loadingTimer = null;
        }
        entry.loading = false;
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
      const boundsKey = browserWebviewBoundsKey(bounds);
      if (entry.boundsKey === boundsKey) return;
      await Promise.all([
        entry.webview.setPosition(new LogicalPosition(bounds.x, bounds.y)),
        entry.webview.setSize(new LogicalSize(bounds.width, bounds.height)),
      ]);
      entry.boundsKey = boundsKey;
    };

    const createWebview = async (
      label: string,
      url: string,
    ): Promise<ManagedWebview> => {
      const existing = await Webview.getByLabel(label);
      if (disposed) {
        if (existing) void retireBrowserLiveWebview(existing);
        throw new Error("实时浏览器 WebView 已停止");
      }
      if (existing) {
        const entry = {
          webview: existing,
          url: "",
          ready: true,
          visible: false,
          loading: false,
          loadingTimer: null,
          boundsKey: "",
          failedUrl: null,
          failedAt: null,
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
        loading: false,
        loadingTimer: null,
        boundsKey: browserWebviewBoundsKey(bounds),
        failedUrl: null,
        failedAt: null,
      };
      managed.set(label, entry);
      setEntryLoading(entry, label, true);

      void webview.once("tauri://created", () => {
        if (disposed) {
          void retireBrowserLiveWebview(webview);
          return;
        }
        entry.ready = true;
        if (entry.failedUrl) return;
        if (occludedRef.current) {
          entry.visible = false;
          void webview.hide().catch(reportError);
        }
        setStatus("loading");
        scheduleRef.current();
      });
      void webview.once<string>("tauri://error", (event) => {
        managed.delete(label);
        if (entry.loadingTimer !== null) {
          window.clearTimeout(entry.loadingTimer);
          entry.loadingTimer = null;
        }
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

      const activeLabel = labelFor(current.sessionId, tab.id);
      const liveLabels = new Set(
        (current.tabs.length ? current.tabs : [tab]).map((item) =>
          labelFor(current.sessionId, item.id),
        ),
      );
      for (const [label, entry] of managed) {
        if (liveLabels.has(label)) continue;
        managed.delete(label);
        if (entry.loadingTimer !== null) {
          window.clearTimeout(entry.loadingTimer);
        }
        entry.visible = false;
        void retireBrowserLiveWebview(entry.webview);
      }

      let entry = managed.get(activeLabel);
      if (!entry)
        entry = await ensureWebview(activeLabel, tab.url || current.url);
      if (disposed) return;

      const previewUrl = tab.url || current.url;
      const pendingNativeUrl = pendingNativeUrls.get(activeLabel);
      if (
        pendingNativeUrl &&
        canonicalBrowserUrl(pendingNativeUrl) ===
          canonicalBrowserUrl(previewUrl)
      ) {
        pendingNativeUrls.delete(activeLabel);
      }
      const desiredUrl = resolveLiveDesiredUrl(
        previewUrl,
        pendingNativeUrls.get(activeLabel),
      );
      if (
        entry.failedUrl &&
        canonicalBrowserUrl(entry.failedUrl) ===
          canonicalBrowserUrl(desiredUrl) &&
        entry.failedAt === current.updatedAt
      ) {
        setStatus("error");
        return;
      }
      entry.failedUrl = null;
      entry.failedAt = null;

      for (const [label, candidate] of managed) {
        if (!candidate.ready) continue;
        const shouldShow =
          !occludedRef.current && label === activeLabel && !candidate.failedUrl;
        if (candidate.visible !== shouldShow) {
          candidate.visible = shouldShow;
          void (
            shouldShow ? candidate.webview.show() : candidate.webview.hide()
          ).catch(reportError);
        }
      }

      if (!entry.ready) return;
      await updateBounds(entry);
      if (disposed) return;

      if (
        desiredUrl &&
        canonicalBrowserUrl(desiredUrl) !== canonicalBrowserUrl(entry.url)
      ) {
        const previousUrl = entry.url;
        entry.url = desiredUrl;
        setEntryLoading(entry, activeLabel, true);
        setStatus("loading");
        try {
          await invoke("browser_live_webview_control", {
            request: {
              label: activeLabel,
              action: "navigate",
              url: desiredUrl,
            },
          });
          if (disposed) return;
        } catch (cause) {
          entry.url = previousUrl;
          setEntryLoading(entry, activeLabel, false);
          entry.failedUrl = desiredUrl;
          entry.failedAt = current.updatedAt;
          reportError(cause);
          return;
        }
      }

      if (current.status === "connected" && current.action === "reload") {
        const reloadKey = `${activeLabel}:${current.updatedAt}`;
        if (reloadKey !== lastReloadKey) {
          lastReloadKey = reloadKey;
          setEntryLoading(entry, activeLabel, true);
          setStatus("loading");
          void invoke("browser_live_webview_control", {
            request: { label: activeLabel, action: "reload" },
          }).catch((cause) => {
            setEntryLoading(entry, activeLabel, false);
            entry.failedUrl = entry.url;
            entry.failedAt = current.updatedAt;
            reportError(cause);
          });
        }
      }
      setStatus(entry.loading ? "loading" : "ready");
    };

    const schedule = () => {
      if (disposed || !listenerReady) return;
      window.cancelAnimationFrame(animationFrame);
      animationFrame = window.requestAnimationFrame(() => {
        if (syncRunning) {
          syncRequested = true;
          return;
        }
        syncRunning = true;
        void (async () => {
          try {
            do {
              syncRequested = false;
              await sync();
            } while (syncRequested && !disposed);
          } finally {
            syncRunning = false;
          }
        })().catch(reportError);
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

    void (async () => {
      await cleanupStaleBrowserLiveWebviews();
      if (disposed) return;
      const stop = await listen<LivePageEvent>(
        LIVE_PAGE_EVENT,
        ({ payload }) => {
          if (disposed) return;
          const entry = managed.get(payload.label);
          if (!entry) return;
          if (payload.status === "blocked") {
            entry.url = "";
            setEntryLoading(entry, payload.label, false);
            entry.failedUrl = payload.url;
            entry.failedAt = previewRef.current?.updatedAt ?? null;
            entry.visible = false;
            void entry.webview.hide().catch(() => undefined);
            reportError(payload.error || "该网址已被浏览器权限设置拦截");
            return;
          }

          entry.url = payload.url;
          setEntryLoading(entry, payload.label, payload.status === "started");
          entry.failedUrl = null;
          entry.failedAt = null;
          const current = previewRef.current;
          const tab = activeTab(current);
          if (
            !current ||
            !tab ||
            payload.label !== labelFor(current.sessionId, tab.id)
          ) {
            return;
          }

          const nativeUrl = canonicalBrowserUrl(payload.url);
          if (nativeUrl && nativeUrl !== canonicalBrowserUrl(current.url)) {
            pendingNativeUrls.set(payload.label, payload.url);
          }
          callbacksRef.current.onUrlChange(payload.url);
          callbacksRef.current.onError(null);
          setStatus(payload.status === "started" ? "loading" : "ready");
          if (payload.status !== "finished") return;
          if (!nativeUrl || nativeUrl === canonicalBrowserUrl(current.url))
            return;
          const syncKey = `${payload.label}:${nativeUrl}`;
          if (syncKey === lastNativeSync) return;
          lastNativeSync = syncKey;
          nativeSyncQueue = nativeSyncQueue
            .catch(() => undefined)
            .then(async () => {
              if (disposed) return;
              try {
                await callbacksRef.current.onNavigate(payload.url);
              } catch (cause) {
                if (pendingNativeUrls.get(payload.label) === payload.url) {
                  pendingNativeUrls.delete(payload.label);
                }
                if (lastNativeSync === syncKey) lastNativeSync = "";
                const message =
                  cause instanceof Error ? cause.message : String(cause);
                callbacksRef.current.onError(message);
              }
            });
        },
      );
      if (disposed) {
        stop();
        return;
      }
      unlisten = stop;
      listenerReady = true;
      schedule();
    })().catch(reportError);
    return () => {
      disposed = true;
      scheduleRef.current = () => undefined;
      window.cancelAnimationFrame(animationFrame);
      window.removeEventListener("resize", schedule);
      window.removeEventListener("scroll", schedule, true);
      observer?.disconnect();
      unlisten?.();
      for (const entry of managed.values()) {
        if (entry.loadingTimer !== null) {
          window.clearTimeout(entry.loadingTimer);
        }
        entry.visible = false;
        void retireBrowserLiveWebview(entry.webview);
      }
      managed.clear();
      pendingCreates.clear();
      pendingNativeUrls.clear();
    };
  }, [active, viewportRef]);

  useLayoutEffect(() => {
    if (!active) return;
    scheduleRef.current();
  }, [
    active,
    occluded,
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
