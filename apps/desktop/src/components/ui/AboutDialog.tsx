/** 关于 Astro：品牌向玻璃对话框（替代系统原生 About）。 */
import { useEffect, useId, useState } from "react";
import { createPortal } from "react-dom";
import { getVersion } from "@tauri-apps/api/app";
import { X } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import { useDynamicOverlayLayer } from "../../hooks/ui/useDynamicOverlayLayer";
import appIcon from "../../assets/astro-app-icon.png";

type Props = {
  open: boolean;
  onClose: () => void;
};

const FEATURES: MessageKey[] = [
  "about.feature.chat",
  "about.feature.memory",
  "about.feature.workspace",
  "about.feature.tools",
  "about.feature.evolution",
];

export default function AboutDialog({ open, onClose }: Props) {
  const { t } = useI18n();
  const titleId = useId();
  const [version, setVersion] = useState("0.1.0");
  const { layer, bringToFront } = useDynamicOverlayLayer(open);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void getVersion()
      .then((v) => {
        if (!cancelled && v) setVersion(v);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  if (!open || typeof document === "undefined") return null;

  return createPortal(
    <div
      className="app-dialog-backdrop about-dialog-backdrop"
      data-app-overlay-layer={layer}
      style={{ zIndex: layer }}
      role="presentation"
      onPointerDownCapture={bringToFront}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="about-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
      >
        <button
          type="button"
          className="about-dialog-close"
          onClick={onClose}
          aria-label={t("about.close")}
        >
          <X size={16} strokeWidth={2.2} aria-hidden />
        </button>

        <div className="about-dialog-hero" aria-hidden>
          <div className="about-dialog-orb">
            <img
              className="about-dialog-icon"
              src={appIcon}
              alt=""
              width={88}
              height={88}
              draggable={false}
            />
          </div>
        </div>

        <h2 id={titleId} className="about-dialog-title">
          Astro
        </h2>
        <p className="about-dialog-tagline">{t("about.tagline")}</p>
        <p className="about-dialog-version">
          {t("about.version", { v: version })}
        </p>

        <p className="about-dialog-body">{t("about.body")}</p>

        <ul className="about-dialog-features">
          {FEATURES.map((key) => (
            <li key={key}>{t(key)}</li>
          ))}
        </ul>

        <button type="button" className="about-dialog-ok" onClick={onClose}>
          {t("about.close")}
        </button>
      </div>
    </div>,
    document.body,
  );
}
