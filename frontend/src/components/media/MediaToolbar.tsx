/** 媒体悬停工具条：引用 / 放大 / 下载 / 复制 */
import { useCallback, useMemo, useState, type MouseEvent } from "react";
import {
  Check,
  Copy,
  Download,
  ExternalLink,
  Maximize2,
  Quote,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useChatMediaAttach } from "../../contexts/ChatMediaAttachContext";
import { useI18n } from "../../i18n/LocaleContext";
import { useTransientToast } from "../../hooks/ui/useTransientToast";
import { displayUserPath } from "../../lib/filespace/displayPath";
import {
  copyMedia,
  downloadMedia,
  type MediaActionKind,
} from "../../lib/media/mediaActions";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import MediaLightbox from "./MediaLightbox";

type Props = {
  path: string;
  kind: MediaActionKind;
  /** 紧凑：更小按钮 */
  compact?: boolean;
  className?: string;
  alt?: string;
};

export default function MediaToolbar({
  path,
  kind,
  compact,
  className,
  alt,
}: Props) {
  const { t } = useI18n();
  const attachApi = useChatMediaAttach();
  const { showToast, toastHost } = useTransientToast();
  const [busy, setBusy] = useState<"download" | "copy" | "quote" | null>(null);
  const [copied, setCopied] = useState(false);
  const [lightboxOpen, setLightboxOpen] = useState(false);

  const previewSrc = useMemo(() => resolveMediaSrc(path), [path]);
  const canQuote =
    (kind === "image" ||
      kind === "html" ||
      kind === "code" ||
      kind === "document") &&
    Boolean(attachApi);
  const canZoom = kind === "image" && Boolean(previewSrc);

  const onQuote = useCallback(
    async (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (busy || !attachApi) return;
      setBusy("quote");
      try {
        await attachApi.attachMediaPath(path);
        showToast(t("media.quoted"), { tone: "success" });
      } catch (err) {
        showToast(
          `${t("media.actionFailed")}${err ? `：${String(err)}` : ""}`,
          { error: true },
        );
      } finally {
        setBusy(null);
      }
    },
    [attachApi, busy, path, showToast, t],
  );

  const onZoom = useCallback(
    (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (!previewSrc) return;
      setLightboxOpen(true);
    },
    [previewSrc],
  );

  const onDownload = useCallback(
    async (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (busy) return;
      setBusy("download");
      try {
        const saved = await downloadMedia(path);
        showToast(
          t("media.downloadSuccess", { path: displayUserPath(saved) }),
          {
            tone: "success",
            durationMs: 8000,
          },
        );
      } catch (err) {
        showToast(
          `${t("media.actionFailed")}${err ? `：${String(err)}` : ""}`,
          { error: true },
        );
      } finally {
        setBusy(null);
      }
    },
    [busy, path, showToast, t],
  );

  const onCopy = useCallback(
    async (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (busy) return;
      setBusy("copy");
      try {
        await copyMedia(path, kind);
        setCopied(true);
        showToast(t("media.copied"), { tone: "success" });
        window.setTimeout(() => setCopied(false), 1600);
      } catch (err) {
        showToast(
          `${t("media.actionFailed")}${err ? `：${String(err)}` : ""}`,
          { error: true },
        );
      } finally {
        setBusy(null);
      }
    },
    [busy, path, kind, showToast, t],
  );

  const onOpenExternal = useCallback(
    (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      void invoke("open_path_externally", { path }).catch(() => {});
    },
    [path],
  );

  const icon = compact ? 14 : 15;
  const quoteLabel = t("media.quote");
  const zoomLabel = t("media.zoom");
  const copyLabel = copied ? t("media.copied") : t("media.copy");
  const dlLabel = t("media.download");
  const openLabel = t("workspace.openExternally");

  return (
    <>
      <div
        className={`media-toolbar ${compact ? "is-compact" : ""} ${className ?? ""}`.trim()}
        role="toolbar"
        aria-label={t("media.actions")}
      >
        {canQuote ? (
          <button
            type="button"
            className="media-toolbar-btn"
            onClick={(e) => void onQuote(e)}
            disabled={busy !== null}
            title={quoteLabel}
            aria-label={quoteLabel}
          >
            <Quote size={icon} strokeWidth={2.1} aria-hidden />
          </button>
        ) : null}
        {canZoom ? (
          <button
            type="button"
            className="media-toolbar-btn"
            onClick={onZoom}
            disabled={busy !== null}
            title={zoomLabel}
            aria-label={zoomLabel}
          >
            <Maximize2 size={icon} strokeWidth={2.1} aria-hidden />
          </button>
        ) : null}
        <button
          type="button"
          className="media-toolbar-btn"
          onClick={onOpenExternal}
          title={openLabel}
          aria-label={openLabel}
        >
          <ExternalLink size={icon} strokeWidth={2.1} aria-hidden />
        </button>
        <button
          type="button"
          className="media-toolbar-btn"
          onClick={(e) => void onDownload(e)}
          disabled={busy !== null}
          title={dlLabel}
          aria-label={dlLabel}
        >
          <Download size={icon} strokeWidth={2.1} aria-hidden />
        </button>
        <button
          type="button"
          className={`media-toolbar-btn ${copied ? "is-copied" : ""}`}
          onClick={(e) => void onCopy(e)}
          disabled={busy !== null}
          title={copyLabel}
          aria-label={copyLabel}
        >
          {copied ? (
            <Check size={icon} strokeWidth={2.4} aria-hidden />
          ) : (
            <Copy size={icon} strokeWidth={2.1} aria-hidden />
          )}
        </button>
      </div>
      {toastHost}
      {lightboxOpen && previewSrc ? (
        <MediaLightbox
          src={previewSrc}
          alt={alt}
          onClose={() => setLightboxOpen(false)}
        />
      ) : null}
    </>
  );
}
