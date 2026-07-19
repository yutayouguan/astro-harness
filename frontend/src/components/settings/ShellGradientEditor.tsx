/** Shell 自定义渐变编辑器：双色点拖拽 + 快捷色 + 确认/取消。 */
import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { createPortal } from "react-dom";
import { useI18n } from "../../i18n/LocaleContext";
import {
  clampPercent,
  cloneGradient,
  SHELL_GRADIENT_SWATCH_COLORS,
  type ShellGradient,
} from "../../lib/ui/shellGradient";

type StopKey = "primary" | "secondary";

type Props = {
  open: boolean;
  initial: ShellGradient;
  onPreview: (gradient: ShellGradient) => void;
  onConfirm: (gradient: ShellGradient) => void;
  onCancel: () => void;
};

export default function ShellGradientEditor({
  open,
  initial,
  onPreview,
  onConfirm,
  onCancel,
}: Props) {
  const { t } = useI18n();
  const titleId = useId();
  const canvasRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<StopKey | null>(null);
  const [draft, setDraft] = useState<ShellGradient>(() => ({
    ...cloneGradient(initial),
    id: "custom",
  }));
  const [active, setActive] = useState<StopKey>("primary");
  const rafRef = useRef<number | null>(null);
  const pendingRef = useRef<ShellGradient | null>(null);

  useEffect(() => {
    if (!open) return;
    const next = { ...cloneGradient(initial), id: "custom" as const };
    setDraft(next);
    setActive("primary");
    onPreview(next);
    // 仅在打开时同步；编辑中 parent 的 preview 回写不应重置 draft
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const flushPreview = useCallback(
    (next: ShellGradient) => {
      pendingRef.current = next;
      if (rafRef.current != null) return;
      rafRef.current = window.requestAnimationFrame(() => {
        rafRef.current = null;
        const g = pendingRef.current;
        if (g) onPreview(g);
      });
    },
    [onPreview],
  );

  useEffect(() => {
    return () => {
      if (rafRef.current != null) window.cancelAnimationFrame(rafRef.current);
    };
  }, []);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onCancel();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onCancel]);

  const updateStop = useCallback(
    (key: StopKey, patch: Partial<ShellGradient["primary"]>) => {
      setDraft((prev) => {
        const next: ShellGradient = {
          id: "custom",
          primary: key === "primary" ? { ...prev.primary, ...patch } : prev.primary,
          secondary:
            key === "secondary" ? { ...prev.secondary, ...patch } : prev.secondary,
        };
        if (patch.x != null) next[key].x = clampPercent(patch.x);
        if (patch.y != null) next[key].y = clampPercent(patch.y);
        flushPreview(next);
        return next;
      });
    },
    [flushPreview],
  );

  const pointFromEvent = (clientX: number, clientY: number) => {
    const el = canvasRef.current;
    if (!el) return null;
    const rect = el.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return null;
    const x = ((clientX - rect.left) / rect.width) * 100;
    const y = ((clientY - rect.top) / rect.height) * 100;
    return { x: clampPercent(x), y: clampPercent(y) };
  };

  const onPointerDownStop = (key: StopKey, e: ReactPointerEvent<HTMLButtonElement>) => {
    e.preventDefault();
    e.stopPropagation();
    setActive(key);
    dragRef.current = key;
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const onPointerMove = (e: ReactPointerEvent<HTMLElement>) => {
    const key = dragRef.current;
    if (!key) return;
    const pt = pointFromEvent(e.clientX, e.clientY);
    if (!pt) return;
    updateStop(key, pt);
  };

  const onPointerUp = (e: ReactPointerEvent<HTMLElement>) => {
    if (dragRef.current && e.currentTarget.hasPointerCapture?.(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
    dragRef.current = null;
  };

  const onCanvasPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.target !== e.currentTarget) return;
    const pt = pointFromEvent(e.clientX, e.clientY);
    if (!pt) return;
    updateStop(active, pt);
  };

  const onStopKeyDown = (key: StopKey, e: ReactKeyboardEvent<HTMLButtonElement>) => {
    const step = e.shiftKey ? 5 : 2;
    let dx = 0;
    let dy = 0;
    if (e.key === "ArrowLeft") dx = -step;
    else if (e.key === "ArrowRight") dx = step;
    else if (e.key === "ArrowUp") dy = -step;
    else if (e.key === "ArrowDown") dy = step;
    else return;
    e.preventDefault();
    setActive(key);
    const stop = draft[key];
    updateStop(key, { x: stop.x + dx, y: stop.y + dy });
  };

  if (!open || typeof document === "undefined") return null;

  return createPortal(
    <div
      className="shell-grad-editor-backdrop"
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
    >
      <div
        className="shell-grad-editor"
        role="dialog"
        aria-modal
        aria-labelledby={titleId}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <header className="shell-grad-editor-head">
          <div>
            <h3 id={titleId} className="shell-grad-editor-title">
              {t("prefs.colorStyle.editorTitle")}
            </h3>
            <p className="shell-grad-editor-sub">{t("prefs.colorStyle.editorSub")}</p>
          </div>
        </header>

        <div
          ref={canvasRef}
          className="shell-grad-canvas"
          style={{
            background: `
              radial-gradient(circle at ${draft.primary.x}% ${draft.primary.y}%, ${draft.primary.color} 0%, transparent 42%),
              radial-gradient(circle at ${draft.secondary.x}% ${draft.secondary.y}%, ${draft.secondary.color} 0%, transparent 48%),
              #e8edf5
            `,
          }}
          onPointerDown={onCanvasPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
        >
          {(["primary", "secondary"] as const).map((key) => {
            const stop = draft[key];
            return (
              <button
                key={key}
                type="button"
                className={`shell-grad-handle ${active === key ? "is-active" : ""} ${
                  key === "secondary" ? "is-secondary" : ""
                }`}
                style={{
                  left: `${stop.x}%`,
                  top: `${stop.y}%`,
                  background: stop.color,
                }}
                aria-label={
                  key === "primary"
                    ? t("prefs.colorStyle.primaryStop")
                    : t("prefs.colorStyle.secondaryStop")
                }
                aria-pressed={active === key}
                onPointerDown={(e) => onPointerDownStop(key, e)}
                onPointerMove={onPointerMove}
                onPointerUp={onPointerUp}
                onKeyDown={(e) => onStopKeyDown(key, e)}
                onClick={() => setActive(key)}
              />
            );
          })}
        </div>

        <div className="shell-grad-palette" role="group" aria-label={t("prefs.colorStyle.swatches")}>
          {SHELL_GRADIENT_SWATCH_COLORS.map((color) => (
            <button
              key={color}
              type="button"
              className={`shell-grad-swatch ${draft[active].color === color ? "is-active" : ""}`}
              style={{ background: color }}
              title={color}
              aria-label={color}
              aria-pressed={draft[active].color === color}
              onClick={() => updateStop(active, { color })}
            />
          ))}
          <label className="shell-grad-custom-color" title={t("prefs.colorStyle.pickColor")}>
            <input
              type="color"
              value={draft[active].color}
              onChange={(e) => updateStop(active, { color: e.target.value })}
              aria-label={t("prefs.colorStyle.pickColor")}
            />
          </label>
        </div>

        <footer className="shell-grad-editor-actions">
          <button type="button" className="shell-grad-btn shell-grad-btn--ghost" onClick={onCancel}>
            {t("prefs.colorStyle.cancel")}
          </button>
          <button
            type="button"
            className="shell-grad-btn shell-grad-btn--primary"
            onClick={() => onConfirm({ ...cloneGradient(draft), id: "custom" })}
          >
            {t("prefs.colorStyle.confirm")}
          </button>
        </footer>
      </div>
    </div>,
    document.body,
  );
}
