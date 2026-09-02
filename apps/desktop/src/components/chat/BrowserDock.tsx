import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type FormEvent,
  type KeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent,
  type TransitionEvent as ReactTransitionEvent,
  type WheelEvent,
} from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import {
  ArrowLeft,
  ArrowRight,
  Camera,
  CircleAlert,
  Download,
  ExternalLink,
  Globe2,
  LoaderCircle,
  Maximize2,
  Minimize2,
  MoreVertical,
  Plus,
  RefreshCw,
  X,
} from "lucide-react";
import type { BrowserPreview } from "../../hooks/chat/useBrowserPreview";
import { useBrowserLiveWebviews } from "../../hooks/chat/useBrowserLiveWebviews";
import { useI18n } from "../../i18n/LocaleContext";
import { normalizeBrowserUrl } from "../../lib/browser/browserUrl";
import {
  BROWSER_DOCK_DEFAULT_WIDTH,
  BROWSER_DOCK_MIN_WIDTH,
  BROWSER_DOCK_OVERLAY_BREAKPOINT,
  BROWSER_DOCK_WIDTH_KEY,
  clampBrowserDockWidth,
  maxBrowserDockWidth,
  parseStoredBrowserDockWidth,
} from "../../lib/ui/browserDockWidth";

type BrowserControl = (
  action: string,
  args?: Record<string, unknown>,
) => Promise<unknown>;

type Props = {
  open: boolean;
  preview: BrowserPreview | null;
  expanded: boolean;
  onControl: BrowserControl;
  onExpandedChange: (expanded: boolean) => void;
  onTitleMouseDown?: (event: ReactMouseEvent) => void;
  onTitleDoubleClick?: (event: ReactMouseEvent) => void;
  onClose: () => void;
};

const RESIZE_KEYBOARD_STEP = 24;
const RESIZE_KEYBOARD_LARGE_STEP = 64;
const RESTORE_ANIMATION_MS = 300;

function formatBytes(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}

export default function BrowserDock({
  open,
  preview,
  expanded,
  onControl,
  onExpandedChange,
  onTitleMouseDown,
  onTitleDoubleClick,
  onClose,
}: Props) {
  const { t } = useI18n();
  const [address, setAddress] = useState(preview?.url ?? "");
  const [downloadsOpen, setDownloadsOpen] = useState(false);
  const [actionsOpen, setActionsOpen] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const screenshotRef = useRef<HTMLImageElement>(null);
  const nativeViewportRef = useRef<HTMLDivElement>(null);
  const dockRef = useRef<HTMLElement>(null);
  const dockWidthRef = useRef(BROWSER_DOCK_DEFAULT_WIDTH);
  const dragRef = useRef<{
    pointerId: number;
    startX: number;
    startWidth: number;
  } | null>(null);
  const actionsRef = useRef<HTMLDivElement>(null);
  const actionsTriggerRef = useRef<HTMLButtonElement>(null);
  const wheelDeltaRef = useRef(0);
  const wheelTimerRef = useRef<number | null>(null);
  const resizeTimerRef = useRef<number | null>(null);
  const restoreTimerRef = useRef<number | null>(null);
  const lastViewportRef = useRef("");
  const controlRunRef = useRef(0);
  const [dockWidth, setDockWidth] = useState(() => {
    try {
      return parseStoredBrowserDockWidth(
        localStorage.getItem(BROWSER_DOCK_WIDTH_KEY),
      );
    } catch {
      return BROWSER_DOCK_DEFAULT_WIDTH;
    }
  });
  const [maxDockWidth, setMaxDockWidth] = useState(BROWSER_DOCK_DEFAULT_WIDTH);
  const [resizing, setResizing] = useState(false);

  const syncLiveNavigation = useCallback(
    async (url: string) => {
      const runId = ++controlRunRef.current;
      setError(null);
      setAddress(url);
      try {
        await onControl("open", { url, new_tab: false });
      } catch (cause) {
        if (controlRunRef.current === runId) {
          setError(cause instanceof Error ? cause.message : String(cause));
        }
        throw cause;
      }
    },
    [onControl],
  );

  const liveWebview = useBrowserLiveWebviews({
    active: open,
    occluded: actionsOpen,
    preview,
    viewportRef: nativeViewportRef,
    onUrlChange: setAddress,
    onNavigate: syncLiveNavigation,
    onError: setError,
  });

  dockWidthRef.current = dockWidth;

  const containerWidth = useCallback(() => {
    const width =
      dockRef.current?.parentElement?.getBoundingClientRect().width ?? 0;
    return width > 0 ? width : Number.POSITIVE_INFINITY;
  }, []);

  const usesOverlayLayout = useCallback(
    () => window.innerWidth <= BROWSER_DOCK_OVERLAY_BREAKPOINT,
    [],
  );

  const updateDockWidth = useCallback(
    (nextWidth: number, persist = false) => {
      const next = clampBrowserDockWidth(
        nextWidth,
        containerWidth(),
        usesOverlayLayout(),
      );
      dockWidthRef.current = next;
      setDockWidth(next);
      if (!persist) return;
      try {
        localStorage.setItem(BROWSER_DOCK_WIDTH_KEY, String(next));
      } catch {
        // Storage is best-effort in private or locked-down webviews.
      }
    },
    [containerWidth, usesOverlayLayout],
  );

  useLayoutEffect(() => {
    const container = dockRef.current?.parentElement;
    if (!container) return;
    const syncBounds = () => {
      const width = container.getBoundingClientRect().width;
      if (width <= 0) return;
      const overlayLayout = usesOverlayLayout();
      const nextMax = maxBrowserDockWidth(width, overlayLayout);
      setMaxDockWidth(nextMax);
      const nextWidth = clampBrowserDockWidth(
        dockWidthRef.current,
        width,
        overlayLayout,
      );
      dockWidthRef.current = nextWidth;
      setDockWidth(nextWidth);
    };
    syncBounds();
    const observer =
      typeof ResizeObserver !== "undefined"
        ? new ResizeObserver(syncBounds)
        : null;
    observer?.observe(container);
    window.addEventListener("resize", syncBounds);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", syncBounds);
    };
  }, [usesOverlayLayout]);

  const finishResize = useCallback((pointerId: number) => {
    if (dragRef.current?.pointerId !== pointerId) return;
    dragRef.current = null;
    setResizing(false);
    try {
      localStorage.setItem(
        BROWSER_DOCK_WIDTH_KEY,
        String(dockWidthRef.current),
      );
    } catch {
      // Storage is best-effort in private or locked-down webviews.
    }
  }, []);

  const onResizePointerDown = (event: PointerEvent<HTMLButtonElement>) => {
    if (event.button !== 0 || dragRef.current) return;
    event.preventDefault();
    dragRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startWidth:
        dockRef.current?.getBoundingClientRect().width ?? dockWidthRef.current,
    };
    setResizing(true);
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const onResizePointerMove = (event: PointerEvent<HTMLButtonElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    updateDockWidth(drag.startWidth + drag.startX - event.clientX);
  };

  const onResizePointerUp = (event: PointerEvent<HTMLButtonElement>) => {
    finishResize(event.pointerId);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const onResizeKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    const step = event.shiftKey
      ? RESIZE_KEYBOARD_LARGE_STEP
      : RESIZE_KEYBOARD_STEP;
    let nextWidth: number | null = null;
    if (event.key === "ArrowLeft") nextWidth = dockWidthRef.current + step;
    else if (event.key === "ArrowRight")
      nextWidth = dockWidthRef.current - step;
    else if (event.key === "Home") nextWidth = BROWSER_DOCK_MIN_WIDTH;
    else if (event.key === "End") nextWidth = maxDockWidth;
    if (nextWidth == null) return;
    event.preventDefault();
    updateDockWidth(nextWidth, true);
  };

  const finishRestore = useCallback(() => {
    if (restoreTimerRef.current != null) {
      window.clearTimeout(restoreTimerRef.current);
      restoreTimerRef.current = null;
    }
    setRestoring(false);
    onExpandedChange(false);
  }, [onExpandedChange]);

  const toggleExpanded = () => {
    if (restoring) {
      if (restoreTimerRef.current != null) {
        window.clearTimeout(restoreTimerRef.current);
        restoreTimerRef.current = null;
      }
      setRestoring(false);
      return;
    }
    if (!expanded) {
      onExpandedChange(true);
      return;
    }
    if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) {
      onExpandedChange(false);
      return;
    }
    setRestoring(true);
    restoreTimerRef.current = window.setTimeout(
      finishRestore,
      RESTORE_ANIMATION_MS + 60,
    );
  };

  const onDockTransitionEnd = (event: ReactTransitionEvent<HTMLElement>) => {
    if (
      !restoring ||
      event.target !== event.currentTarget ||
      event.propertyName !== "width"
    )
      return;
    finishRestore();
  };

  useEffect(() => {
    if (preview?.url) setAddress(preview.url);
  }, [preview?.url]);

  useEffect(() => {
    if (!actionsOpen) return;
    const closeOnOutsidePointer = (event: globalThis.PointerEvent) => {
      if (!actionsRef.current?.contains(event.target as Node)) {
        setActionsOpen(false);
      }
    };
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setActionsOpen(false);
      actionsTriggerRef.current?.focus();
    };
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsidePointer);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [actionsOpen]);

  useEffect(() => {
    if (!open) setActionsOpen(false);
  }, [open]);

  useEffect(() => {
    if (expanded) return;
    setRestoring(false);
    if (restoreTimerRef.current != null) {
      window.clearTimeout(restoreTimerRef.current);
      restoreTimerRef.current = null;
    }
  }, [expanded]);

  useEffect(
    () => () => {
      if (wheelTimerRef.current != null)
        window.clearTimeout(wheelTimerRef.current);
      if (resizeTimerRef.current != null)
        window.clearTimeout(resizeTimerRef.current);
      if (restoreTimerRef.current != null)
        window.clearTimeout(restoreTimerRef.current);
    },
    [],
  );

  useEffect(() => {
    const viewport = nativeViewportRef.current;
    if (!viewport || !preview?.activeTabId || preview.status !== "connected")
      return;
    const observer = new ResizeObserver(([entry]) => {
      const width = Math.round(entry.contentRect.width);
      const height = Math.round(entry.contentRect.height);
      if (width < 1 || height < 1) return;
      const key = `${width}x${height}:${preview.activeTabId}`;
      if (key === lastViewportRef.current) return;
      if (resizeTimerRef.current != null)
        window.clearTimeout(resizeTimerRef.current);
      resizeTimerRef.current = window.setTimeout(() => {
        lastViewportRef.current = key;
        void onControl("resize", {
          width,
          height,
          screenshot: !liveWebview.isLive,
        }).catch((cause) => {
          setError(cause instanceof Error ? cause.message : String(cause));
        });
      }, 180);
    });
    observer.observe(viewport);
    return () => {
      observer.disconnect();
      if (resizeTimerRef.current != null) {
        window.clearTimeout(resizeTimerRef.current);
        resizeTimerRef.current = null;
      }
    };
  }, [liveWebview.isLive, onControl, preview?.activeTabId]);

  const run = async (action: string, args: Record<string, unknown> = {}) => {
    const runId = ++controlRunRef.current;
    setError(null);
    try {
      await onControl(action, args);
      if (controlRunRef.current === runId) setError(null);
    } catch (cause) {
      if (controlRunRef.current === runId) {
        setError(cause instanceof Error ? cause.message : String(cause));
      }
    }
  };

  const submitAddress = (event: FormEvent) => {
    event.preventDefault();
    const url = normalizeBrowserUrl(address);
    if (!url) return;
    setAddress(url);
    void run("open", { url, new_tab: false });
  };

  const clickPreview = (event: PointerEvent<HTMLImageElement>) => {
    const image = screenshotRef.current;
    if (!image || !image.naturalWidth || !image.naturalHeight) return;
    const rect = image.getBoundingClientRect();
    const scale = Math.min(
      rect.width / image.naturalWidth,
      rect.height / image.naturalHeight,
    );
    const renderedWidth = image.naturalWidth * scale;
    const renderedHeight = image.naturalHeight * scale;
    const offsetX = (rect.width - renderedWidth) / 2;
    const localX = event.clientX - rect.left - offsetX;
    const localY = event.clientY - rect.top;
    if (
      localX < 0 ||
      localY < 0 ||
      localX > renderedWidth ||
      localY > renderedHeight
    )
      return;
    const x = (localX / renderedWidth) * image.naturalWidth;
    const y = (localY / renderedHeight) * image.naturalHeight;
    event.currentTarget.focus();
    void run("click_point", { x, y, wait_ms: 300 });
  };

  const scrollPreview = (event: WheelEvent<HTMLImageElement>) => {
    event.preventDefault();
    wheelDeltaRef.current += event.deltaY;
    if (wheelTimerRef.current != null) return;
    wheelTimerRef.current = window.setTimeout(() => {
      const y = Math.round(wheelDeltaRef.current);
      wheelDeltaRef.current = 0;
      wheelTimerRef.current = null;
      void run("scroll", { y });
    }, 100);
  };

  const keyPreview = (event: KeyboardEvent<HTMLImageElement>) => {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    if (
      event.key.length !== 1 &&
      !/^(Enter|Backspace|Tab|Escape|Arrow(?:Left|Right|Up|Down))$/.test(
        event.key,
      )
    )
      return;
    event.preventDefault();
    void run("key", { key: event.key });
  };

  const screenshot = preview?.screenshotPath
    ? preview.screenshotPath.startsWith("data:") ||
      preview.screenshotPath.startsWith("http")
      ? preview.screenshotPath
      : `${convertFileSrc(preview.screenshotPath)}?v=${preview.updatedAt}`
    : null;

  return (
    <aside
      ref={dockRef}
      className={`browser-dock${open ? " is-open" : ""}${resizing ? " is-resizing" : ""}${expanded ? " is-expanded" : ""}${restoring ? " is-restoring" : ""}`}
      aria-label={t("chat.browserDock.title")}
      aria-hidden={!open}
      style={{ "--browser-dock-width": `${dockWidth}px` } as CSSProperties}
      onTransitionEnd={onDockTransitionEnd}
    >
      {!expanded ? (
        <button
          type="button"
          className="browser-dock-resizer"
          role="separator"
          aria-label={t("chat.browserDock.resize")}
          aria-orientation="vertical"
          aria-valuemin={Math.min(BROWSER_DOCK_MIN_WIDTH, maxDockWidth)}
          aria-valuemax={maxDockWidth}
          aria-valuenow={dockWidth}
          title={t("chat.browserDock.resize")}
          onDoubleClick={() =>
            updateDockWidth(BROWSER_DOCK_DEFAULT_WIDTH, true)
          }
          onKeyDown={onResizeKeyDown}
          onPointerDown={onResizePointerDown}
          onPointerMove={onResizePointerMove}
          onPointerUp={onResizePointerUp}
          onPointerCancel={(event) => finishResize(event.pointerId)}
          onLostPointerCapture={(event) => finishResize(event.pointerId)}
        />
      ) : null}
      <div
        className="browser-dock-drag-region"
        onMouseDown={onTitleMouseDown}
        onDoubleClick={onTitleDoubleClick}
        aria-hidden
      />
      <header className="browser-dock-header">
        <div
          className="browser-tabs"
          role="tablist"
          aria-label={t("chat.browserDock.tabs")}
        >
          <div className="browser-tabs-scroll">
            {(preview?.tabs ?? []).map((tab) => (
              <div
                key={tab.id}
                role="tab"
                tabIndex={0}
                aria-selected={tab.active}
                className={`browser-tab${tab.active ? " is-active" : ""}`}
                title={tab.url}
                onClick={() => void run("switch_tab", { tab_id: tab.id })}
                onKeyDown={(event) => {
                  if (event.key !== "Enter" && event.key !== " ") return;
                  event.preventDefault();
                  void run("switch_tab", { tab_id: tab.id });
                }}
              >
                <Globe2 size={12} aria-hidden />
                <span>
                  {tab.title || tab.url || t("chat.browserDock.newTab")}
                </span>
                <button
                  type="button"
                  aria-label={`${t("chat.browserDock.closeTab")} ${tab.title || ""}`}
                  onClick={(event) => {
                    event.stopPropagation();
                    void run("close_tab", { tab_id: tab.id });
                  }}
                  onKeyDown={(event) => {
                    if (event.key !== "Enter" && event.key !== " ") return;
                    event.preventDefault();
                    event.stopPropagation();
                    void run("close_tab", { tab_id: tab.id });
                  }}
                >
                  <X size={11} aria-hidden />
                </button>
              </div>
            ))}
          </div>
        </div>
      </header>

      <form className="browser-address-row" onSubmit={submitAddress}>
        <button
          type="button"
          aria-label={t("chat.browserDock.back")}
          disabled={!preview}
          onClick={() => void run("back")}
        >
          <ArrowLeft size={14} aria-hidden />
        </button>
        <button
          type="button"
          aria-label={t("chat.browserDock.forward")}
          disabled={!preview}
          onClick={() => void run("forward")}
        >
          <ArrowRight size={14} aria-hidden />
        </button>
        <button
          type="button"
          aria-label={t("chat.browserDock.reload")}
          disabled={!preview}
          onClick={() => void run("reload")}
        >
          {liveWebview.isLoading ? (
            <LoaderCircle className="browser-spin" size={14} aria-hidden />
          ) : (
            <RefreshCw size={14} aria-hidden />
          )}
        </button>
        <input
          value={address}
          onChange={(event) => setAddress(event.target.value)}
          placeholder={t("chat.browserDock.addressPlaceholder")}
          aria-label={t("chat.browserDock.address")}
          spellCheck={false}
        />
        <button
          type="submit"
          className="browser-address-go"
          disabled={!address.trim()}
        >
          {t("chat.browserDock.open")}
        </button>
        <button
          type="button"
          aria-label={t("chat.browserDock.openExternal")}
          disabled={!preview?.url}
          onClick={() => {
            if (!preview?.url) return;
            void import("@tauri-apps/plugin-opener").then(({ openUrl }) =>
              openUrl(preview.url),
            );
          }}
        >
          <ExternalLink size={14} aria-hidden />
        </button>
        <button
          type="button"
          aria-label={t("chat.browserDock.screenshot")}
          disabled={!preview}
          onClick={() => void run("snapshot", { screenshot: true })}
        >
          <Camera size={14} aria-hidden />
        </button>
        <button
          type="button"
          className="browser-dock-expand"
          aria-label={t(
            expanded && !restoring
              ? "chat.browserDock.restore"
              : "chat.browserDock.expand",
          )}
          title={t(
            expanded && !restoring
              ? "chat.browserDock.restore"
              : "chat.browserDock.expand",
          )}
          aria-pressed={expanded && !restoring}
          onClick={toggleExpanded}
        >
          {expanded && !restoring ? (
            <Minimize2 size={14} aria-hidden />
          ) : (
            <Maximize2 size={14} aria-hidden />
          )}
        </button>
        <div className="browser-dock-overflow" ref={actionsRef}>
          <button
            ref={actionsTriggerRef}
            type="button"
            className={`browser-dock-overflow-trigger${actionsOpen ? " is-active" : ""}`}
            aria-label={t("chat.browserDock.moreActions")}
            title={t("chat.browserDock.moreActions")}
            aria-haspopup="menu"
            aria-expanded={actionsOpen}
            aria-controls="browser-dock-actions-menu"
            onClick={() => setActionsOpen((open) => !open)}
          >
            <MoreVertical size={15} aria-hidden />
          </button>
          {actionsOpen ? (
            <div
              id="browser-dock-actions-menu"
              className="browser-dock-actions-menu"
              role="menu"
              aria-label={t("chat.browserDock.moreActions")}
            >
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  setActionsOpen(false);
                  void run("new_tab");
                }}
              >
                <Plus size={14} aria-hidden />
                <span>{t("chat.browserDock.newTab")}</span>
              </button>
              <button
                type="button"
                role="menuitem"
                disabled={!preview}
                onClick={() => {
                  setActionsOpen(false);
                  setDownloadsOpen((open) => !open);
                  void run("downloads");
                }}
              >
                <Download size={14} aria-hidden />
                <span>{t("chat.browserDock.downloads")}</span>
                {preview?.downloads.length ? (
                  <small>{preview.downloads.length}</small>
                ) : null}
              </button>
              <button
                type="button"
                role="menuitem"
                className="is-danger"
                onClick={() => {
                  setActionsOpen(false);
                  onClose();
                }}
              >
                <X size={14} aria-hidden />
                <span>{t("chat.browserDock.close")}</span>
              </button>
            </div>
          ) : null}
        </div>
        {error && liveWebview.isLive ? (
          <span
            className="browser-address-error"
            role="alert"
            title={error}
            aria-label={error}
          >
            <CircleAlert size={14} aria-hidden />
          </span>
        ) : null}
      </form>

      {downloadsOpen ? (
        <div className="browser-downloads">
          <strong>{t("chat.browserDock.downloads")}</strong>
          {preview?.downloads.length ? (
            preview.downloads.map((download) => (
              <button
                key={download.path}
                type="button"
                onClick={() =>
                  void invoke("open_path_externally", { path: download.path })
                }
              >
                <span>{download.name}</span>
                <small>
                  {download.status === "downloading"
                    ? t("chat.browserDock.downloading")
                    : formatBytes(download.size)}
                </small>
              </button>
            ))
          ) : (
            <span className="browser-downloads-empty">
              {t("chat.browserDock.noDownloads")}
            </span>
          )}
        </div>
      ) : null}

      <div className="browser-viewport">
        <div
          ref={nativeViewportRef}
          className="browser-native-viewport"
          aria-hidden
        />
        {!liveWebview.isLive && preview?.status === "connecting" ? (
          <div className="browser-viewport-status">
            <LoaderCircle className="browser-spin" size={20} aria-hidden />
            {t("chat.browserDock.loading")}
          </div>
        ) : null}
        {liveWebview.isLive && preview?.url && !actionsOpen ? (
          <>
            {screenshot ? (
              <img
                className="browser-live-underlay"
                src={screenshot}
                alt=""
                aria-hidden
                draggable={false}
              />
            ) : null}
            <div
              className={`browser-live-placeholder${screenshot ? " has-underlay" : ""}`}
              aria-hidden
            />
          </>
        ) : screenshot ? (
          <img
            ref={screenshotRef}
            src={screenshot}
            alt={preview?.title || t("chat.browserPreview.title")}
            tabIndex={0}
            draggable={false}
            onPointerDown={clickPreview}
            onWheel={scrollPreview}
            onKeyDown={keyPreview}
          />
        ) : (
          <div className="browser-empty">
            <Globe2 size={28} aria-hidden />
            <strong>{t("chat.browserDock.emptyReady")}</strong>
            <span>{t("chat.browserDock.supports")}</span>
          </div>
        )}
        {error && !liveWebview.isLive ? (
          <div className="browser-error">{error}</div>
        ) : null}
      </div>
    </aside>
  );
}
