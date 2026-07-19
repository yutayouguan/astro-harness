/** Shell 自定义渐变编辑器：2–5 个色点拖拽 + 快捷色 + 确认/取消。 */
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
  gradientStops,
  gradientWithStops,
  SHELL_GRADIENT_SWATCH_COLORS,
  type ShellGradient,
  type ShellGradientStop,
} from "../../lib/ui/shellGradient";

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
  const dragRef = useRef<number | null>(null);
  const [draft, setDraft] = useState<ShellGradient>(() => ({
    ...cloneGradient(initial),
    id: "custom",
  }));
  const [active, setActive] = useState(0);
  const rafRef = useRef<number | null>(null);
  const pendingRef = useRef<ShellGradient | null>(null);

  useEffect(() => {
    if (!open) return;
    const next = { ...cloneGradient(initial), id: "custom" as const };
    setDraft(next);
    setActive(0);
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
    (index: number, patch: Partial<ShellGradientStop>) => {
      setDraft((prev) => {
        const stops = gradientStops(prev);
        const current = stops[index];
        if (!current) return prev;
        stops[index] = {
          ...current,
          ...patch,
          x: patch.x != null ? clampPercent(patch.x) : current.x,
          y: patch.y != null ? clampPercent(patch.y) : current.y,
        };
        const next = gradientWithStops("custom", stops);
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

  const onPointerDownStop = (index: number, e: ReactPointerEvent<HTMLButtonElement>) => {
    e.preventDefault();
    e.stopPropagation();
    setActive(index);
    dragRef.current = index;
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const onPointerMove = (e: ReactPointerEvent<HTMLElement>) => {
    const index = dragRef.current;
    if (index == null) return;
    const pt = pointFromEvent(e.clientX, e.clientY);
    if (!pt) return;
    updateStop(index, pt);
  };

  const onPointerUp = (e: ReactPointerEvent<HTMLElement>) => {
    if (dragRef.current != null && e.currentTarget.hasPointerCapture?.(e.pointerId)) {
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

  const onStopKeyDown = (index: number, e: ReactKeyboardEvent<HTMLButtonElement>) => {
    const step = e.shiftKey ? 5 : 2;
    let dx = 0;
    let dy = 0;
    if (e.key === "ArrowLeft") dx = -step;
    else if (e.key === "ArrowRight") dx = step;
    else if (e.key === "ArrowUp") dy = -step;
    else if (e.key === "ArrowDown") dy = step;
    else return;
    e.preventDefault();
    setActive(index);
    const stop = gradientStops(draft)[index];
    if (!stop) return;
    updateStop(index, { x: stop.x + dx, y: stop.y + dy });
  };

  const addStop = () => {
    const stops = gradientStops(draft);
    if (stops.length >= 5) return;
    const index = stops.length;
    const color =
      SHELL_GRADIENT_SWATCH_COLORS[(index + 2) % SHELL_GRADIENT_SWATCH_COLORS.length];
    const offsets = [
      { x: 50, y: 50 },
      { x: 35, y: 65 },
      { x: 68, y: 62 },
    ];
    const pos = offsets[index - 2] ?? offsets[0];
    stops.push({ color, ...pos });
    const next = gradientWithStops("custom", stops);
    setDraft(next);
    setActive(stops.length - 1);
    flushPreview(next);
  };

  const removeStop = () => {
    const stops = gradientStops(draft);
    if (stops.length <= 2) return;
    stops.splice(active, 1);
    const next = gradientWithStops("custom", stops);
    setDraft(next);
    setActive(Math.min(active, stops.length - 1));
    flushPreview(next);
  };

  if (!open || typeof document === "undefined") return null;

  const stops = gradientStops(draft);
  const selected = stops[active] ?? stops[0];

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
            background: `${stops
              .map(
                (stop, index) =>
                  `radial-gradient(circle at ${stop.x}% ${stop.y}%, ${stop.color} 0%, transparent ${index === 0 ? 42 : 48}%)`,
              )
              .join(", ")}, #e8edf5`,
          }}
          onPointerDown={onCanvasPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
        >
          {stops.map((stop, index) => (
            <button
              key={index}
              type="button"
              className={`shell-grad-handle ${active === index ? "is-active" : ""} ${
                index === 1 ? "is-secondary" : ""
              }`}
              style={{
                left: `${stop.x}%`,
                top: `${stop.y}%`,
                background: stop.color,
              }}
              aria-label={t("prefs.colorStyle.colorStop", {
                index: String(index + 1),
              })}
              aria-pressed={active === index}
              onPointerDown={(e) => onPointerDownStop(index, e)}
              onPointerMove={onPointerMove}
              onPointerUp={onPointerUp}
              onKeyDown={(e) => onStopKeyDown(index, e)}
              onClick={() => setActive(index)}
            />
          ))}

          <div
            className="shell-grad-stop-actions"
            aria-label={t("prefs.colorStyle.stopCount")}
            onPointerDown={(e) => e.stopPropagation()}
          >
            <button
              type="button"
              className="shell-grad-stop-btn"
              disabled={stops.length <= 2}
              onClick={removeStop}
              aria-label={t("prefs.colorStyle.removeStop")}
            >
              −
            </button>
            <button
              type="button"
              className="shell-grad-stop-btn"
              disabled={stops.length >= 5}
              onClick={addStop}
              aria-label={t("prefs.colorStyle.addStop")}
            >
              +
            </button>
          </div>
        </div>

        <div className="shell-grad-palette" role="group" aria-label={t("prefs.colorStyle.swatches")}>
          {SHELL_GRADIENT_SWATCH_COLORS.map((color) => (
            <button
              key={color}
              type="button"
              className={`shell-grad-swatch ${selected?.color === color ? "is-active" : ""}`}
              style={{ background: color }}
              title={color}
              aria-label={color}
              aria-pressed={selected?.color === color}
              onClick={() => updateStop(active, { color })}
            />
          ))}
          <label className="shell-grad-custom-color" title={t("prefs.colorStyle.pickColor")}>
            <input
              type="color"
              value={selected?.color ?? "#2563eb"}
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
