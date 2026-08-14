/** Agent 头像选择：右侧抽屉（预设插画 + 上传）。 */
import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import { CoverPicker, type CoverId } from "../../illustrations";

export type AvatarPickerDrawerProps = {
  open: boolean;
  coverId: CoverId | null;
  busy?: boolean;
  onClose: () => void;
  onPickCover: (id: CoverId) => void;
  onUpload: (file: File) => void;
};

export default function AvatarPickerDrawer({
  open,
  coverId,
  busy,
  onClose,
  onPickCover,
  onUpload,
}: AvatarPickerDrawerProps) {
  const { t } = useI18n();
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, busy, onClose]);

  if (!open) return null;

  return createPortal(
    <div
      className="agent-icon-drawer-backdrop"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      <div
        className="agent-icon-drawer"
        role="dialog"
        aria-modal="true"
        aria-labelledby="agent-avatar-drawer-title"
      >
        <header className="agent-icon-drawer-head">
          <div>
            <h3 id="agent-avatar-drawer-title" className="lucide-picker-title">
              {t("chat.agentAvatarDrawerTitle")}
            </h3>
            <p className="lucide-picker-sub">{t("chat.agentAvatarDrawerSub")}</p>
          </div>
          <button
            type="button"
            className="lucide-picker-close"
            onClick={onClose}
            disabled={busy}
            aria-label={t("chat.lucidePickerClose")}
          >
            <X size={16} />
          </button>
        </header>
        <div className="agent-icon-drawer-scroll">
          <CoverPicker
            value={coverId}
            busy={busy}
            onChange={onPickCover}
          />
        </div>
        <footer className="agent-icon-drawer-foot">
          <p className="chat-agent-covers-sub">{t("chat.agentAvatarDrawerUploadHint")}</p>
          <button
            type="button"
            className="chat-agent-icon-btn"
            disabled={busy}
            onClick={() => inputRef.current?.click()}
          >
            {t("chat.agentAvatarDrawerUpload")}
          </button>
          <input
            ref={inputRef}
            type="file"
            accept="image/png,image/jpeg,image/webp,image/gif,image/svg+xml,.png,.jpg,.jpeg,.webp,.gif,.svg"
            hidden
            onChange={(e) => {
              const file = e.target.files?.[0] ?? null;
              e.target.value = "";
              if (file) onUpload(file);
            }}
          />
        </footer>
      </div>
    </div>,
    document.body,
  );
}
