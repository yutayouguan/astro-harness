import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent,
} from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  ExternalLink,
  Globe2,
  GripHorizontal,
  Loader2,
  Unplug,
  X,
} from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import type { BrowserPreview } from "../../hooks/chat/useBrowserPreview";

type Props = {
  preview: BrowserPreview;
  onClose: () => void;
};

type DragState = {
  pointerId: number;
  startX: number;
  startY: number;
  originX: number;
  originY: number;
  panelRect: DOMRect;
  parentRect: DOMRect;
};

export default function BrowserPreviewFloat({ preview, onClose }: Props) {
  const { t } = useI18n();
  const panelRef = useRef<HTMLElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const [offset, setOffset] = useState({ x: 0, y: 0 });

  useEffect(() => {
    setOffset({ x: 0, y: 0 });
  }, [preview.sessionId]);

  const onPointerDown = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (event.button !== 0 || !panelRef.current?.parentElement) return;
      const target = event.target as HTMLElement;
      if (target.closest("button,a")) return;
      event.currentTarget.setPointerCapture(event.pointerId);
      dragRef.current = {
        pointerId: event.pointerId,
        startX: event.clientX,
        startY: event.clientY,
        originX: offset.x,
        originY: offset.y,
        panelRect: panelRef.current.getBoundingClientRect(),
        parentRect: panelRef.current.parentElement.getBoundingClientRect(),
      };
    },
    [offset.x, offset.y],
  );

  const onPointerMove = useCallback((event: PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const dx = event.clientX - drag.startX;
    const dy = event.clientY - drag.startY;
    const margin = 8;
    const minDx = drag.parentRect.left + margin - drag.panelRect.left;
    const maxDx = drag.parentRect.right - margin - drag.panelRect.right;
    const minDy = drag.parentRect.top + margin - drag.panelRect.top;
    const maxDy = drag.parentRect.bottom - margin - drag.panelRect.bottom;
    setOffset({
      x: drag.originX + Math.min(maxDx, Math.max(minDx, dx)),
      y: drag.originY + Math.min(maxDy, Math.max(minDy, dy)),
    });
  }, []);

  const stopDrag = useCallback((event: PointerEvent<HTMLDivElement>) => {
    if (dragRef.current?.pointerId === event.pointerId) dragRef.current = null;
  }, []);

  const openExternal = useCallback(() => {
    if (!preview.url) return;
    try {
      const url = new URL(preview.url);
      if (url.protocol !== "http:" && url.protocol !== "https:") return;
      void import("@tauri-apps/plugin-opener")
        .then(({ openUrl }) => openUrl(url.toString()))
        .catch(() => undefined);
    } catch {
      // Ignore malformed persisted URLs.
    }
  }, [preview.url]);

  const statusKey = `chat.browserPreview.${preview.status}` as const;
  const screenshot = preview.screenshotPath
    ? preview.screenshotPath.startsWith("data:") ||
      preview.screenshotPath.startsWith("http")
      ? preview.screenshotPath
      : `${convertFileSrc(preview.screenshotPath)}?v=${preview.updatedAt}`
    : null;

  return (
    <aside
      ref={panelRef}
      className={`browser-preview-float is-${preview.status}`}
      style={{ transform: `translate3d(${offset.x}px, ${offset.y}px, 0)` }}
      aria-label={t("chat.browserPreview.title")}
    >
      <div
        className="browser-preview-head"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={stopDrag}
        onPointerCancel={stopDrag}
      >
        <span className="browser-preview-brand" aria-hidden>
          <Globe2 size={14} />
        </span>
        <span className="browser-preview-heading">
          <strong>{preview.title || t("chat.browserPreview.title")}</strong>
          <small title={preview.url}>
            {preview.url || t("chat.browserPreview.connecting")}
          </small>
        </span>
        <span className={`browser-preview-status is-${preview.status}`}>
          {preview.status === "connecting" ? (
            <Loader2 size={11} className="browser-preview-spin" aria-hidden />
          ) : null}
          {t(statusKey)}
        </span>
        <GripHorizontal
          className="browser-preview-grip"
          size={14}
          aria-hidden
        />
        <button
          type="button"
          onClick={openExternal}
          disabled={!preview.url}
          title={t("chat.browserPreview.openExternal")}
          aria-label={t("chat.browserPreview.openExternal")}
        >
          <ExternalLink size={14} />
        </button>
        <button
          type="button"
          onClick={onClose}
          title={t("chat.browserPreview.close")}
          aria-label={t("chat.browserPreview.close")}
        >
          <X size={14} />
        </button>
      </div>
      <div className="browser-preview-body">
        {screenshot ? <img src={screenshot} alt="" draggable={false} /> : null}
        {!screenshot ||
        preview.status === "disconnected" ||
        preview.status === "error" ? (
          <div className="browser-preview-placeholder">
            {preview.status === "disconnected" ? (
              <Unplug size={24} aria-hidden />
            ) : (
              <Globe2 size={24} aria-hidden />
            )}
            <span>{t(statusKey)}</span>
            {preview.status === "disconnected" ? (
              <small>{t("chat.browserPreview.restoreHint")}</small>
            ) : null}
          </div>
        ) : null}
      </div>
    </aside>
  );
}
