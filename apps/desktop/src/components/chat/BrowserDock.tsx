import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
  type PointerEvent,
  type WheelEvent,
} from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import {
  ArrowLeft,
  ArrowRight,
  Camera,
  Download,
  ExternalLink,
  Globe2,
  LoaderCircle,
  Plus,
  RefreshCw,
  X,
} from "lucide-react";
import type { BrowserPreview } from "../../hooks/chat/useBrowserPreview";
import { useI18n } from "../../i18n/LocaleContext";
import { normalizeBrowserUrl } from "../../lib/browser/browserUrl";

type BrowserControl = (
  action: string,
  args?: Record<string, unknown>,
) => Promise<unknown>;

type Props = {
  preview: BrowserPreview | null;
  onControl: BrowserControl;
  onClose: () => void;
};

function formatBytes(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / (1024 * 1024)).toFixed(1)} MB`;
}

export default function BrowserDock({ preview, onControl, onClose }: Props) {
  const { t } = useI18n();
  const [address, setAddress] = useState(preview?.url ?? "");
  const [downloadsOpen, setDownloadsOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const screenshotRef = useRef<HTMLImageElement>(null);
  const viewportRef = useRef<HTMLDivElement>(null);
  const wheelDeltaRef = useRef(0);
  const wheelTimerRef = useRef<number | null>(null);
  const resizeTimerRef = useRef<number | null>(null);
  const lastViewportRef = useRef("");

  useEffect(() => {
    if (preview?.url) setAddress(preview.url);
  }, [preview?.url]);

  useEffect(
    () => () => {
      if (wheelTimerRef.current != null)
        window.clearTimeout(wheelTimerRef.current);
      if (resizeTimerRef.current != null)
        window.clearTimeout(resizeTimerRef.current);
    },
    [],
  );

  useEffect(() => {
    const viewport = viewportRef.current;
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
        void onControl("resize", { width, height }).catch((cause) => {
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
  }, [onControl, preview?.activeTabId]);

  const run = async (action: string, args: Record<string, unknown> = {}) => {
    setError(null);
    try {
      await onControl(action, args);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
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
    <aside className="browser-dock" aria-label={t("chat.browserDock.title")}>
      <header className="browser-dock-header">
        <strong>
          <Globe2 size={15} aria-hidden />
          {t("chat.browserDock.title")}
        </strong>
        <div className="browser-dock-header-actions">
          <button
            type="button"
            aria-label={t("chat.browserDock.downloads")}
            disabled={!preview}
            className={downloadsOpen ? "is-active" : undefined}
            onClick={() => {
              setDownloadsOpen((open) => !open);
              void run("downloads");
            }}
          >
            <Download size={14} aria-hidden />
            {preview?.downloads.length ? (
              <span>{preview.downloads.length}</span>
            ) : null}
          </button>
          <button
            type="button"
            aria-label={t("chat.browserDock.close")}
            onClick={onClose}
          >
            <X size={15} aria-hidden />
          </button>
        </div>
      </header>

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
        <button
          type="button"
          className="browser-new-tab"
          aria-label={t("chat.browserDock.newTab")}
          onClick={() => void run("new_tab")}
        >
          <Plus size={14} aria-hidden />
        </button>
      </div>

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
          <RefreshCw size={14} aria-hidden />
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

      <div className="browser-viewport" ref={viewportRef}>
        {preview?.status === "connecting" ? (
          <div className="browser-viewport-status">
            <LoaderCircle className="browser-spin" size={20} aria-hidden />
            {t("chat.browserDock.loading")}
          </div>
        ) : null}
        {screenshot ? (
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
        {error ? <div className="browser-error">{error}</div> : null}
      </div>
    </aside>
  );
}
