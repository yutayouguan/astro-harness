/** 图片全屏放大预览（Esc / 点击遮罩关闭） */
import { useEffect, useId } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";

type Props = {
  src: string;
  alt?: string;
  onClose: () => void;
};

export default function MediaLightbox({ src, alt, onClose }: Props) {
  const { t } = useI18n();
  const titleId = useId();

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

  return createPortal(
    <div
      className="media-lightbox"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      onClick={onClose}
    >
      <p id={titleId} className="sr-only">
        {t("media.zoom")}
      </p>
      <button
        type="button"
        className="media-lightbox-close"
        onClick={onClose}
        aria-label={t("media.zoomClose")}
      >
        <X size={20} strokeWidth={2.2} aria-hidden />
      </button>
      <img
        className="media-lightbox-image"
        src={src}
        alt={alt ?? ""}
        onClick={(e) => e.stopPropagation()}
      />
    </div>,
    document.body,
  );
}
