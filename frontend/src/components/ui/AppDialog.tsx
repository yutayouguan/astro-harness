/** 应用内确认 / 输入对话框（Portal 到 body，避免被侧栏裁切）。 */
import { useEffect, useId, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { AlertTriangle, Info, Pencil } from "lucide-react";

export type AppDialogVariant = "default" | "danger" | "prompt";

export type AppDialogProps = {
  open: boolean;
  title: string;
  message?: string;
  variant?: AppDialogVariant;
  confirmLabel: string;
  cancelLabel: string;
  confirmDisabled?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  children?: ReactNode;
};

export default function AppDialog({
  open,
  title,
  message,
  variant = "default",
  confirmLabel,
  cancelLabel,
  confirmDisabled = false,
  onConfirm,
  onCancel,
  children,
}: AppDialogProps) {
  const titleId = useId();
  const panelRef = useRef<HTMLDivElement | null>(null);
  const confirmRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const prev = document.activeElement as HTMLElement | null;
    const t = window.setTimeout(() => {
      const input = panelRef.current?.querySelector<HTMLInputElement>(
        "input:not([type='hidden']):not([disabled])",
      );
      if (input) {
        input.focus();
        input.select();
      } else {
        confirmRef.current?.focus();
      }
    }, 0);
    return () => {
      window.clearTimeout(t);
      prev?.focus?.();
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onCancel();
        return;
      }
      if (event.key === "Enter" && !event.isComposing) {
        const target = event.target as HTMLElement | null;
        if (target?.tagName === "TEXTAREA") return;
        if (confirmDisabled) return;
        event.preventDefault();
        onConfirm();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onCancel, onConfirm, confirmDisabled]);

  if (!open || typeof document === "undefined") return null;

  const toneClass =
    variant === "danger"
      ? "is-danger"
      : variant === "prompt"
        ? "is-prompt"
        : "is-default";

  return createPortal(
    <div
      className="app-dialog-backdrop"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
    >
      <div
        ref={panelRef}
        className={`app-dialog ${toneClass}`}
        role="dialog"
        aria-modal
        aria-labelledby={titleId}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="app-dialog-head">
          <span className="app-dialog-icon" aria-hidden>
            {variant === "danger" ? (
              <AlertTriangle size={18} strokeWidth={1.9} />
            ) : variant === "prompt" ? (
              <Pencil size={17} strokeWidth={1.9} />
            ) : (
              <Info size={18} strokeWidth={1.9} />
            )}
          </span>
          <div className="app-dialog-copy">
            <h3 id={titleId}>{title}</h3>
            {message ? <p>{message}</p> : null}
          </div>
        </div>
        {children ? <div className="app-dialog-body">{children}</div> : null}
        <div className="app-dialog-actions">
          <button
            type="button"
            className="app-dialog-btn is-cancel"
            onClick={onCancel}
          >
            {cancelLabel}
          </button>
          <button
            ref={confirmRef}
            type="button"
            className={`app-dialog-btn is-confirm ${
              variant === "danger" ? "is-danger" : ""
            }`}
            disabled={confirmDisabled}
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
