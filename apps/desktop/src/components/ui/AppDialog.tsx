/** 应用内确认 / 输入对话框（Portal 到 body，避免被侧栏裁切）。 */
import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { AlertTriangle, Info, Pencil } from "lucide-react";
import { useDynamicOverlayLayer } from "../../hooks/ui/useDynamicOverlayLayer";

export type AppDialogVariant = "default" | "danger" | "prompt";

export type AppDialogProps = {
  open: boolean;
  title: string;
  message?: string;
  /** 醒目对象名（如待删会话标题），显示在标题与说明之间。 */
  emphasis?: string;
  emphasisLabel?: string;
  variant?: AppDialogVariant;
  confirmLabel: string;
  cancelLabel: string;
  confirmDisabled?: boolean;
  /** Destructive flows can require explicit button activation, not a global Enter shortcut. */
  confirmOnEnter?: boolean;
  trapFocus?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  children?: ReactNode;
};

export default function AppDialog({
  open,
  title,
  message,
  emphasis,
  emphasisLabel,
  variant = "default",
  confirmLabel,
  cancelLabel,
  confirmDisabled = false,
  confirmOnEnter = true,
  trapFocus = false,
  onConfirm,
  onCancel,
  children,
}: AppDialogProps) {
  const titleId = useId();
  const panelRef = useRef<HTMLDivElement | null>(null);
  const confirmRef = useRef<HTMLButtonElement | null>(null);
  const backdropRef = useRef<HTMLDivElement | null>(null);
  const [closing, setClosing] = useState(false);
  const closingRef = useRef(false);
  const { layer, bringToFront } = useDynamicOverlayLayer(open);

  const startClose = useCallback((action: () => void) => {
    if (closingRef.current) return;
    const bd = backdropRef.current;
    if (!bd) {
      action();
      return;
    }
    setClosing(true);
    closingRef.current = true;
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      bd.removeEventListener("animationend", finish);
      setClosing(false);
      closingRef.current = false;
      action();
    };
    bd.addEventListener("animationend", finish);
    setTimeout(finish, 150);
  }, []);

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
      if (trapFocus && event.key === "Tab") {
        const panel = panelRef.current;
        const items = panel
          ? Array.from(
              panel.querySelectorAll<HTMLElement>(
                "button:not([disabled]), input:not([disabled]), [tabindex='0']",
              ),
            )
          : [];
        const first = items[0],
          last = items[items.length - 1];
        if (
          first &&
          last &&
          (!panel?.contains(document.activeElement) ||
            (event.shiftKey && document.activeElement === first) ||
            (!event.shiftKey && document.activeElement === last))
        ) {
          event.preventDefault();
          (event.shiftKey ? last : first).focus();
        }
      }
      if (event.key === "Escape") {
        event.preventDefault();
        startClose(onCancel);
        return;
      }
      if (confirmOnEnter && event.key === "Enter" && !event.isComposing) {
        const target = event.target as HTMLElement | null;
        if (target?.tagName === "TEXTAREA") return;
        if (confirmDisabled) return;
        event.preventDefault();
        startClose(onConfirm);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    open,
    onCancel,
    onConfirm,
    confirmDisabled,
    confirmOnEnter,
    trapFocus,
    startClose,
  ]);

  if (!open || typeof document === "undefined") return null;

  const toneClass =
    variant === "danger"
      ? "is-danger"
      : variant === "prompt"
        ? "is-prompt"
        : "is-default";

  return createPortal(
    <div
      ref={backdropRef}
      className={`app-dialog-backdrop${closing ? " is-closing" : ""}`}
      data-app-overlay-layer={layer}
      style={{ zIndex: layer }}
      role="presentation"
      onPointerDownCapture={bringToFront}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) startClose(onCancel);
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
            {emphasis ? (
              <div className="app-dialog-emphasis">
                {emphasisLabel ? (
                  <span className="app-dialog-emphasis-label">
                    {emphasisLabel}
                  </span>
                ) : null}
                <strong className="app-dialog-emphasis-value">
                  {emphasis}
                </strong>
              </div>
            ) : null}
            {message ? <p>{message}</p> : null}
          </div>
        </div>
        {children ? <div className="app-dialog-body">{children}</div> : null}
        <div className="app-dialog-actions">
          <button
            type="button"
            className="app-dialog-btn is-cancel"
            disabled={closing}
            onClick={() => startClose(onCancel)}
          >
            {cancelLabel}
          </button>
          <button
            ref={confirmRef}
            type="button"
            className={`app-dialog-btn is-confirm ${
              variant === "danger" ? "is-danger" : ""
            }`}
            disabled={confirmDisabled || closing}
            onClick={() => startClose(onConfirm)}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
