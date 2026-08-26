/** Lucide 图标选择器。 */
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { useMorphicons } from "../../hooks/app/useMorphicons";
import { useI18n } from "../../i18n/LocaleContext";
import {
  applyPaintToSvg,
  DEFAULT_LUCIDE_ICON_COLOR,
  DEFAULT_LUCIDE_PAINT,
  DEFAULT_LUCIDE_RENDER_STYLE,
  filterLucideAgentIcons,
  gradientPaint,
  LUCIDE_ICON_COLORS,
  LUCIDE_ICON_GRADIENTS,
  paintCssBackground,
  paintsEqual,
  solidPaint,
  type LucideAgentIcon,
  type LucideIconComponent,
  type LucidePaint,
  type LucideRenderStyle,
} from "../../lib/agent/lucideAgentIcons";
import { AppMorphIcon } from "../icons/MorphIcon";

/** Lucide 图标选择器入参 */
type Props = {
  open: boolean;
  /** 当前已选图标 id */
  selectedId?: string | null;
  /** 初始填充色/渐变 */
  initialPaint?: LucidePaint;
  /** 初始渲染风格（描边/填充等） */
  initialStyle?: LucideRenderStyle;
  onClose: () => void;
  onSelect: (icon: LucideAgentIcon, paint: LucidePaint, style: LucideRenderStyle) => void;
  /** 若提供，抽屉内显示上传按钮并回调 File */
  onUploadImage?: (file: File) => void;
  /** 异步操作进行中时禁止关闭与上传 */
  busy?: boolean;
  /** portal 主题色继承（跨 tab 场景） */
  toneStyle?: CSSProperties;
};

function paintKey(paint: LucidePaint): string {
  if (paint.kind === "solid") return `s:${paint.color}`;
  return `g:${paint.id}:${paint.from}:${paint.to}:${paint.angle}`;
}

function PaintedLucideIcon({
  Icon,
  paint,
  style,
  size = 20,
}: {
  Icon: LucideIconComponent;
  paint: LucidePaint;
  style: LucideRenderStyle;
  size?: number;
}) {
  const { strokeWidth } = useMorphicons();
  const wrapRef = useRef<HTMLSpanElement | null>(null);
  // fillCutout 会改写 SVG 结构，换色/换样式时必须 remount 拿回原始 path
  const remountKey = `${style}:${paintKey(paint)}:${size}`;

  useLayoutEffect(() => {
    const svg = wrapRef.current?.querySelector("svg");
    if (!svg) return;
    applyPaintToSvg(svg, paint, style);
  }, [paint, style, Icon, size, remountKey]);

  return (
    <span ref={wrapRef} className="lucide-picker-painted" aria-hidden>
      <Icon
        key={remountKey}
        size={size}
        strokeWidth={strokeWidth}
        color={paint.kind === "solid" ? paint.color : "#0f172a"}
      />
    </span>
  );
}

export default function LucideIconPicker({
  open,
  selectedId,
  initialPaint,
  initialStyle,
  onClose,
  onSelect,
  onUploadImage,
  busy,
  toneStyle,
}: Props) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [paint, setPaint] = useState<LucidePaint>(initialPaint ?? DEFAULT_LUCIDE_PAINT);
  const [renderStyle, setRenderStyle] = useState<LucideRenderStyle>(
    initialStyle ?? DEFAULT_LUCIDE_RENDER_STYLE,
  );
  const [customFrom, setCustomFrom] = useState("#2563eb");
  const [customTo, setCustomTo] = useState("#06b6d4");
  const [previewIconId, setPreviewIconId] = useState(selectedId ?? "bot");
  const inputRef = useRef<HTMLInputElement | null>(null);
  const uploadRef = useRef<HTMLInputElement | null>(null);

  const icons = useMemo(() => filterLucideAgentIcons(query), [query]);

  useEffect(() => {
    if (!open) return;
    setQuery("");
    setPreviewIconId(selectedId ?? "bot");
    const next = initialPaint ?? DEFAULT_LUCIDE_PAINT;
    setPaint(next);
    setRenderStyle(initialStyle ?? DEFAULT_LUCIDE_RENDER_STYLE);
    if (next.kind === "gradient" && next.id === "custom") {
      setCustomFrom(next.from);
      setCustomTo(next.to);
    }
    const timer = window.setTimeout(() => inputRef.current?.focus(), 40);
    return () => window.clearTimeout(timer);
  }, [open, selectedId, initialPaint, initialStyle]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, busy, onClose]);

  if (!open) return null;

  const customGradient: LucidePaint = {
    kind: "gradient",
    id: "custom",
    from: customFrom,
    to: customTo,
    angle: 135,
  };
  const previewIcon =
    icons.find((item) => item.id === previewIconId) ??
    filterLucideAgentIcons("").find((item) => item.id === previewIconId) ??
    icons[0];

  return createPortal(
    <div
      className="agent-icon-drawer-backdrop"
      role="presentation"
      style={toneStyle}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
    >
      <div
        className="agent-icon-drawer lucide-picker-drawer"
        role="dialog"
        aria-modal="true"
        aria-label={t("chat.lucidePickerTitle")}
      >
        <header className="agent-icon-drawer-head lucide-picker-head">
          <div>
            <h3 className="lucide-picker-title">{t("chat.lucidePickerTitle")}</h3>
            <p className="lucide-picker-sub">{t("chat.lucidePickerSub")}</p>
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

        <div className="lucide-picker-body">
          {/* 固定区：上传 / 搜索 / 预览 / 配色 */}
          <div className="lucide-picker-controls">
            {onUploadImage ? (
              <div className="lucide-picker-upload-wrap">
                <button
                  type="button"
                  className="chat-agent-icon-btn"
                  disabled={busy}
                  onClick={() => uploadRef.current?.click()}
                >
                  {t("chat.agentIconUpload")}
                </button>
                <input
                  ref={uploadRef}
                  type="file"
                  accept="image/png,image/jpeg,image/webp,image/gif,image/svg+xml,.png,.jpg,.jpeg,.webp,.gif,.svg"
                  hidden
                  disabled={busy}
                  onChange={(e) => {
                    const file = e.target.files?.[0] ?? null;
                    e.target.value = "";
                    if (file) onUploadImage(file);
                  }}
                />
              </div>
            ) : null}

            <div className="lucide-picker-search-wrap">
              <input
                ref={inputRef}
                className="lucide-picker-search"
                type="search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("chat.lucidePickerSearch")}
                aria-label={t("chat.lucidePickerSearch")}
              />
            </div>

            {previewIcon ? (
              <div className="lucide-picker-morph-preview" aria-live="polite">
                <span className="lucide-picker-morph-stage" aria-hidden>
                  {renderStyle === "stroke" ? (
                    <AppMorphIcon
                      icon={previewIcon.data}
                      size={34}
                      color={paint.kind === "solid" ? paint.color : "currentColor"}
                    />
                  ) : (
                    <PaintedLucideIcon
                      Icon={previewIcon.Icon}
                      paint={paint}
                      style={renderStyle}
                      size={34}
                    />
                  )}
                </span>
                <span className="lucide-picker-morph-name">{previewIcon.label}</span>
              </div>
            ) : null}

            <div className="lucide-picker-style" role="group" aria-label={t("chat.lucidePickerStyle")}>
              <span className="lucide-picker-colors-label">{t("chat.lucidePickerStyle")}</span>
              <div className="lucide-picker-style-toggle">
                <button
                  type="button"
                  className={renderStyle === "stroke" ? "is-active" : ""}
                  aria-pressed={renderStyle === "stroke"}
                  onClick={() => setRenderStyle("stroke")}
                >
                  {t("chat.lucidePickerStyleStroke")}
                </button>
                <button
                  type="button"
                  className={renderStyle === "fillCutout" ? "is-active" : ""}
                  aria-pressed={renderStyle === "fillCutout"}
                  onClick={() => setRenderStyle("fillCutout")}
                >
                  {t("chat.lucidePickerStyleFillCutout")}
                </button>
              </div>
            </div>

            <div className="lucide-picker-colors" role="group" aria-label={t("chat.lucidePickerColor")}>
              <span className="lucide-picker-colors-label">{t("chat.lucidePickerColor")}</span>
              <div className="lucide-picker-swatches">
                {LUCIDE_ICON_COLORS.map((c) => {
                  const next = solidPaint(c.value);
                  return (
                    <button
                      key={c.id}
                      type="button"
                      className={`lucide-picker-swatch ${paintsEqual(paint, next) ? "is-active" : ""}`}
                      style={{ background: c.value }}
                      title={c.label}
                      aria-label={c.label}
                      aria-pressed={paintsEqual(paint, next)}
                      onClick={() => setPaint(next)}
                    />
                  );
                })}
                <label className="lucide-picker-custom-color" title={t("chat.lucidePickerCustomColor")}>
                  <input
                    type="color"
                    value={
                      paint.kind === "solid" && /^#[0-9a-fA-F]{6}$/.test(paint.color)
                        ? paint.color
                        : DEFAULT_LUCIDE_ICON_COLOR
                    }
                    onChange={(e) => setPaint(solidPaint(e.target.value))}
                    aria-label={t("chat.lucidePickerCustomColor")}
                  />
                </label>
              </div>
            </div>

            <div
              className="lucide-picker-colors lucide-picker-gradients"
              role="group"
              aria-label={t("chat.lucidePickerGradient")}
            >
              <span className="lucide-picker-colors-label">{t("chat.lucidePickerGradient")}</span>
              <div className="lucide-picker-swatches">
                {LUCIDE_ICON_GRADIENTS.map((g) => {
                  const next = gradientPaint(g);
                  return (
                    <button
                      key={g.id}
                      type="button"
                      className={`lucide-picker-swatch lucide-picker-swatch--grad ${
                        paintsEqual(paint, next) ? "is-active" : ""
                      }`}
                      style={{ background: paintCssBackground(next) }}
                      title={g.label}
                      aria-label={g.label}
                      aria-pressed={paintsEqual(paint, next)}
                      onClick={() => setPaint(next)}
                    />
                  );
                })}
                <button
                  type="button"
                  className={`lucide-picker-swatch lucide-picker-swatch--grad ${
                    paint.kind === "gradient" && paint.id === "custom" ? "is-active" : ""
                  }`}
                  style={{ background: paintCssBackground(customGradient) }}
                  title={t("chat.lucidePickerCustomGradient")}
                  aria-label={t("chat.lucidePickerCustomGradient")}
                  aria-pressed={paint.kind === "gradient" && paint.id === "custom"}
                  onClick={() => setPaint(customGradient)}
                />
              </div>
            </div>

            {paint.kind === "gradient" && paint.id === "custom" ? (
              <div className="lucide-picker-custom-grad-row">
                <label className="lucide-picker-custom-grad-stop">
                  <span>{t("chat.lucidePickerGradFrom")}</span>
                  <input
                    type="color"
                    value={customFrom}
                    onChange={(e) => {
                      const from = e.target.value;
                      setCustomFrom(from);
                      setPaint({ kind: "gradient", id: "custom", from, to: customTo, angle: 135 });
                    }}
                  />
                </label>
                <label className="lucide-picker-custom-grad-stop">
                  <span>{t("chat.lucidePickerGradTo")}</span>
                  <input
                    type="color"
                    value={customTo}
                    onChange={(e) => {
                      const to = e.target.value;
                      setCustomTo(to);
                      setPaint({ kind: "gradient", id: "custom", from: customFrom, to, angle: 135 });
                    }}
                  />
                </label>
              </div>
            ) : null}
          </div>

          {/* 滚动区：仅图标网格 */}
          <div className="lucide-picker-gridwrap">
            <div className="lucide-picker-grid" role="listbox" aria-label={t("chat.lucidePickerTitle")}>
              {icons.length === 0 ? (
                <p className="lucide-picker-empty">{t("chat.lucidePickerEmpty")}</p>
              ) : (
                icons.map((item) => {
                  const active = selectedId === item.id;
                  return (
                    <button
                      key={item.id}
                      type="button"
                      role="option"
                      aria-selected={active}
                      className={`lucide-picker-item ${active ? "is-active" : ""}`}
                      title={item.label}
                      onPointerEnter={() => setPreviewIconId(item.id)}
                      onFocus={() => setPreviewIconId(item.id)}
                      onClick={() => {
                        setPreviewIconId(item.id);
                        onSelect(item, paint, renderStyle);
                      }}
                    >
                      <PaintedLucideIcon
                        Icon={item.Icon}
                        paint={paint}
                        style={renderStyle}
                        size={20}
                      />
                      <span className="lucide-picker-item-label">{item.label}</span>
                    </button>
                  );
                })
              )}
            </div>
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}
