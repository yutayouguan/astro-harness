/** App 内全屏媒体预览（网页/图片/视频/音频，自适应）；Esc / 点击遮罩关闭 */
import { useEffect, useId, useMemo, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import type { MediaActionKind } from "../../lib/media/mediaActions";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import CodeFileCard from "./CodeFileCard";
import GlassAudioPlayer from "./GlassAudioPlayer";
import HtmlPreview from "./HtmlPreview";

type Props = {
  path: string;
  kind: MediaActionKind;
  alt?: string;
  onClose: () => void;
};

export default function MediaPreviewModal({ path, kind, alt, onClose }: Props) {
  const { t } = useI18n();
  const titleId = useId();
  const src = useMemo(() => resolveMediaSrc(path), [path]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    const prev = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      window.removeEventListener("keydown", onKey);
      document.body.style.overflow = prev;
    };
  }, [onClose]);

  let body: ReactNode = null;
  if (kind === "html") {
    body = <HtmlPreview path={path} className="media-preview-modal-html" />;
  } else if (kind === "code") {
    body = <CodeFileCard path={path} className="media-preview-modal-code" />;
  } else if (kind === "video" && src) {
    body = (
      <video
        className="media-preview-modal-video"
        src={src}
        controls
        autoPlay
        playsInline
      />
    );
  } else if (kind === "audio" && src) {
    body = <GlassAudioPlayer src={src} className="media-preview-modal-audio" />;
  } else if (src) {
    body = (
      <img className="media-preview-modal-image" src={src} alt={alt ?? ""} />
    );
  }

  return createPortal(
    <div
      className="media-preview-modal"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      onClick={onClose}
    >
      <p id={titleId} className="sr-only">
        {t("filespace.preview")}
      </p>
      <button
        type="button"
        className="media-preview-modal-close"
        onClick={onClose}
        aria-label={t("media.zoomClose")}
      >
        <X size={20} strokeWidth={2.2} aria-hidden />
      </button>
      <div
        className="media-preview-modal-stage"
        onClick={(e) => e.stopPropagation()}
      >
        {body}
      </div>
    </div>,
    document.body,
  );
}
