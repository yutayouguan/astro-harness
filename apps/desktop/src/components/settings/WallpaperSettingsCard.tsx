import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import {
  Check,
  Image as ImageIcon,
  Images,
  Loader2,
  MonitorUp,
  Palette,
  Plus,
  Sparkles,
  Upload,
  X,
} from "lucide-react";

import type { WallpaperController } from "../../hooks/app/useWallpaper";
import { useI18n } from "../../i18n/LocaleContext";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import {
  DEFAULT_WALLPAPER_HIGHLIGHT_COLOR,
  DEFAULT_WALLPAPER_THEME_COLOR,
  resolveWallpaperPalette,
} from "../../lib/ui/wallpaper";

type Props = {
  controller: WallpaperController;
  tone: string;
};

const STYLE_PROMPTS: Record<string, string> = {
  natural: "自然摄影，真实质感，柔和光线",
  abstract: "抽象流体艺术，克制的渐变和层次",
  minimal: "极简插画，留白充足，低视觉噪声",
  cinematic: "电影感画面，细腻光影，宽幅构图",
};

function WallpaperPreview({
  path,
  name,
  fit,
  shade,
  onError,
}: {
  path: string;
  name: string;
  fit: WallpaperController["prefs"]["fit"];
  shade: number;
  onError: () => void;
}) {
  const src = resolveMediaSrc(path);
  return (
    <div className="wallpaper-preview">
      {src ? (
        <img
          src={src}
          alt=""
          style={{ objectFit: fit === "stretch" ? "fill" : fit }}
          onError={onError}
        />
      ) : null}
      <span
        className="wallpaper-preview-shade"
        style={{ opacity: shade / 100 }}
        aria-hidden
      />
      <span className="wallpaper-preview-shell" aria-hidden>
        <i />
        <b>
          <em />
          <em />
          <strong />
        </b>
      </span>
      <span className="wallpaper-preview-name">{name}</span>
    </div>
  );
}

export default function WallpaperSettingsCard({ controller, tone }: Props) {
  const { t } = useI18n();
  const { prefs, busy, error } = controller;
  const [dialogOpen, setDialogOpen] = useState(false);
  const [prompt, setPrompt] = useState("");
  const [style, setStyle] = useState("natural");
  const [applied, setApplied] = useState(false);
  const automaticPalette = resolveWallpaperPalette(
    { ...prefs, adaptiveColor: true },
    prefs.current,
  );
  const manualPalette = resolveWallpaperPalette(
    { ...prefs, adaptiveColor: false },
    prefs.current,
  );
  const palette = prefs.adaptiveColor
    ? automaticPalette
    : (manualPalette ?? automaticPalette);
  const themeColor = palette?.themeColor ?? DEFAULT_WALLPAPER_THEME_COLOR;
  const highlightColor =
    palette?.highlightColor ?? DEFAULT_WALLPAPER_HIGHLIGHT_COLOR;

  useEffect(() => {
    if (!applied) return;
    const timer = window.setTimeout(() => setApplied(false), 2600);
    return () => window.clearTimeout(timer);
  }, [applied]);

  useEffect(() => {
    if (!dialogOpen || busy) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setDialogOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dialogOpen, busy]);

  const generatedPrompt = useMemo(
    () =>
      `${prompt.trim()}\n风格：${STYLE_PROMPTS[style]}。横向桌面壁纸构图，主体避开中央阅读区域，不要文字、标志或水印。`,
    [prompt, style],
  );

  async function chooseImage() {
    controller.clearError();
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        directory: false,
        title: t("prefs.wallpaper.upload"),
        filters: [
          {
            name: t("prefs.wallpaper.imageFiles"),
            extensions: ["png", "jpg", "jpeg", "webp"],
          },
        ],
      });
      const path = typeof selected === "string" ? selected : null;
      if (!path) return;
      await controller.importImage(path);
      setApplied(true);
    } catch {
      // controller 持有可展示错误；取消系统选择器不需要提示。
    }
  }

  async function generate() {
    if (!prompt.trim() || busy) return;
    try {
      await controller.generate(generatedPrompt);
      setDialogOpen(false);
      setApplied(true);
    } catch {
      // 保留当前壁纸并在卡片/对话框中显示 controller.error。
    }
  }

  const dialog = dialogOpen
    ? createPortal(
        <div
          className="wallpaper-dialog-backdrop"
          role="presentation"
          onMouseDown={(event) => {
            if (event.currentTarget === event.target && !busy) {
              setDialogOpen(false);
            }
          }}
        >
          <section
            className="wallpaper-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="wallpaper-dialog-title"
            data-tone={tone}
          >
            {busy === "generate" ? (
              <div className="wallpaper-generating" role="status">
                <Loader2 size={34} className="wallpaper-spinner" />
                <div>
                  <h2 id="wallpaper-dialog-title">
                    {t("prefs.wallpaper.generating")}
                  </h2>
                  <p>{t("prefs.wallpaper.generatingHint")}</p>
                </div>
                <span className="wallpaper-progress" aria-hidden>
                  <i />
                </span>
              </div>
            ) : (
              <>
                <header className="wallpaper-dialog-head">
                  <span
                    className="prefs-icon-badge"
                    data-tone={tone}
                    aria-hidden
                  >
                    <Sparkles size={20} />
                  </span>
                  <div>
                    <h2 id="wallpaper-dialog-title">
                      {t("prefs.wallpaper.aiTitle")}
                    </h2>
                    <p>{t("prefs.wallpaper.aiSub")}</p>
                  </div>
                  <button
                    type="button"
                    className="wallpaper-dialog-close"
                    aria-label={t("common.close")}
                    onClick={() => setDialogOpen(false)}
                  >
                    <X size={16} />
                  </button>
                </header>
                <label
                  className="wallpaper-field-label"
                  htmlFor="wallpaper-prompt"
                >
                  {t("prefs.wallpaper.prompt")}
                </label>
                <textarea
                  id="wallpaper-prompt"
                  className="wallpaper-prompt"
                  value={prompt}
                  onChange={(event) => setPrompt(event.target.value)}
                  placeholder={t("prefs.wallpaper.promptPlaceholder")}
                  maxLength={4000}
                  autoFocus
                />
                <div className="wallpaper-style-options">
                  {Object.keys(STYLE_PROMPTS).map((id) => (
                    <button
                      key={id}
                      type="button"
                      className={style === id ? "is-active" : ""}
                      aria-pressed={style === id}
                      onClick={() => setStyle(id)}
                    >
                      {t(`prefs.wallpaper.style.${id}` as never)}
                    </button>
                  ))}
                </div>
                <div className="wallpaper-auto-apply">
                  <Check size={15} aria-hidden />
                  <span>{t("prefs.wallpaper.autoApply")}</span>
                  <small>{t("prefs.wallpaper.failureKeepsCurrent")}</small>
                </div>
                {error ? <p className="wallpaper-error">{error}</p> : null}
                <footer className="wallpaper-dialog-actions">
                  <button
                    type="button"
                    className="wallpaper-secondary-button"
                    onClick={() => setDialogOpen(false)}
                  >
                    {t("common.cancel")}
                  </button>
                  <button
                    type="button"
                    className="wallpaper-primary-button"
                    disabled={!prompt.trim()}
                    onClick={() => void generate()}
                  >
                    <Sparkles size={15} />
                    {t("prefs.wallpaper.generateAndApply")}
                  </button>
                </footer>
              </>
            )}
          </section>
        </div>,
        document.body,
      )
    : null;

  return (
    <>
      <section className="prefs-card wallpaper-settings-card">
        <div className="prefs-card-head">
          <div className="prefs-icon-badge" data-tone={tone} aria-hidden>
            <Images size={22} />
          </div>
          <div>
            <h2 className="prefs-card-title">{t("prefs.wallpaper.title")}</h2>
            <p className="prefs-card-sub">{t("prefs.wallpaper.sub")}</p>
          </div>
        </div>

        <div className="wallpaper-source-options" role="radiogroup">
          <button
            type="button"
            role="radio"
            aria-checked={prefs.mode === "color"}
            className={`theme-option ${prefs.mode === "color" ? "active" : ""}`}
            data-tone={tone}
            onClick={() => controller.setMode("color")}
          >
            <span className="theme-option-icon" aria-hidden>
              <Palette />
            </span>
            <span className="theme-option-text">
              <span className="theme-option-label">
                {t("prefs.wallpaper.colorMode")}
              </span>
              <span className="theme-option-desc">
                {t("prefs.wallpaper.colorModeDesc")}
              </span>
            </span>
            <span className="theme-option-check" aria-hidden />
          </button>
          <button
            type="button"
            role="radio"
            aria-checked={prefs.mode === "wallpaper"}
            className={`theme-option ${prefs.mode === "wallpaper" ? "active" : ""}`}
            data-tone={tone}
            onClick={() => controller.setMode("wallpaper")}
          >
            <span className="theme-option-icon" aria-hidden>
              <ImageIcon />
            </span>
            <span className="theme-option-text">
              <span className="theme-option-label">
                {t("prefs.wallpaper.imageMode")}
              </span>
              <span className="theme-option-desc">
                {t("prefs.wallpaper.imageModeDesc")}
              </span>
            </span>
            <span className="theme-option-check" aria-hidden />
          </button>
        </div>

        <div className="wallpaper-editor" hidden={prefs.mode !== "wallpaper"}>
          <div className="wallpaper-preview-column">
            <div className="wallpaper-preview-toggles">
              <button
                type="button"
                role="switch"
                className="wallpaper-preview-toggle"
                aria-checked={prefs.followSystemWallpaper}
                aria-label={t("prefs.wallpaper.followSystem")}
                title={t("prefs.wallpaper.followSystemDesc")}
                onClick={() =>
                  controller.setFollowSystemWallpaper(
                    !prefs.followSystemWallpaper,
                  )
                }
              >
                <MonitorUp size={13} aria-hidden />
                <span>{t("prefs.wallpaper.followSystem")}</span>
                <i aria-hidden />
              </button>
              <button
                type="button"
                role="switch"
                className="wallpaper-preview-toggle"
                aria-checked={prefs.adaptiveColor}
                aria-label={t("prefs.wallpaper.adaptiveColor")}
                title={t("prefs.wallpaper.adaptiveColorDesc")}
                onClick={() =>
                  controller.setAdaptiveColor(!prefs.adaptiveColor)
                }
              >
                <span
                  className="wallpaper-preview-toggle-swatch"
                  style={{
                    background: `linear-gradient(135deg, ${themeColor}, ${highlightColor})`,
                  }}
                  aria-hidden
                />
                <span>{t("prefs.wallpaper.adaptiveColor")}</span>
                <i aria-hidden />
              </button>
            </div>
            {prefs.current ? (
              <WallpaperPreview
                path={prefs.current.path}
                name={prefs.current.name}
                fit={prefs.fit}
                shade={prefs.shade}
                onError={controller.markCurrentUnavailable}
              />
            ) : (
              <button
                type="button"
                className="wallpaper-empty-preview"
                onClick={() => void chooseImage()}
              >
                <Upload size={22} />
                <strong>{t("prefs.wallpaper.emptyTitle")}</strong>
                <span>{t("prefs.wallpaper.emptySub")}</span>
              </button>
            )}
          </div>

          <div className="wallpaper-controls">
            <div className="wallpaper-action-row">
              <button
                type="button"
                className="wallpaper-secondary-button"
                disabled={busy !== null}
                onClick={() => void chooseImage()}
              >
                {busy === "upload" ? (
                  <Loader2 size={15} className="wallpaper-spinner" />
                ) : (
                  <Upload size={15} />
                )}
                {t("prefs.wallpaper.upload")}
              </button>
              <button
                type="button"
                className="wallpaper-primary-button"
                disabled={busy !== null}
                onClick={() => {
                  controller.clearError();
                  setDialogOpen(true);
                }}
              >
                <Sparkles size={15} />
                {t("prefs.wallpaper.aiGenerate")}
              </button>
            </div>

            <div className="wallpaper-control-group wallpaper-palette-control">
              <span className="wallpaper-control-label">
                <span>{t("prefs.wallpaper.palette")}</span>
                <output>
                  {t(
                    prefs.adaptiveColor
                      ? "prefs.wallpaper.paletteAuto"
                      : "prefs.wallpaper.paletteCustom",
                  )}
                </output>
              </span>
              <div className="wallpaper-color-fields">
                <label>
                  <input
                    type="color"
                    value={themeColor}
                    aria-label={t("prefs.wallpaper.themeColor")}
                    onChange={(event) =>
                      controller.setPalette(event.target.value, highlightColor)
                    }
                  />
                  <span>
                    <strong>{t("prefs.wallpaper.themeColor")}</strong>
                    <code>{themeColor}</code>
                  </span>
                </label>
                <label>
                  <input
                    type="color"
                    value={highlightColor}
                    aria-label={t("prefs.wallpaper.highlightColor")}
                    onChange={(event) =>
                      controller.setPalette(themeColor, event.target.value)
                    }
                  />
                  <span>
                    <strong>{t("prefs.wallpaper.highlightColor")}</strong>
                    <code>{highlightColor}</code>
                  </span>
                </label>
              </div>
              <small className="wallpaper-palette-hint">
                {t(
                  prefs.adaptiveColor
                    ? "prefs.wallpaper.paletteAutoDesc"
                    : "prefs.wallpaper.paletteCustomDesc",
                )}
              </small>
            </div>

            <div className="wallpaper-control-group">
              <span className="wallpaper-control-label">
                {t("prefs.wallpaper.fit")}
              </span>
              <div className="wallpaper-fit-options">
                {(["cover", "contain", "stretch"] as const).map((fit) => (
                  <button
                    key={fit}
                    type="button"
                    className={prefs.fit === fit ? "is-active" : ""}
                    aria-pressed={prefs.fit === fit}
                    onClick={() => controller.setFit(fit)}
                  >
                    {t(`prefs.wallpaper.fit.${fit}` as never)}
                  </button>
                ))}
              </div>
            </div>

            <label className="wallpaper-control-group">
              <span className="wallpaper-control-label">
                <span>{t("prefs.wallpaper.shade")}</span>
                <output>{prefs.shade}%</output>
              </span>
              <input
                type="range"
                min="0"
                max="55"
                value={prefs.shade}
                onChange={(event) =>
                  controller.setShade(Number(event.target.value))
                }
              />
            </label>

            <label className="wallpaper-control-group">
              <span className="wallpaper-control-label">
                <span>{t("prefs.wallpaper.blur")}</span>
                <output>
                  {prefs.blur === 0
                    ? t("prefs.wallpaper.off")
                    : `${prefs.blur}px`}
                </output>
              </span>
              <input
                type="range"
                min="0"
                max="12"
                value={prefs.blur}
                onChange={(event) =>
                  controller.setBlur(Number(event.target.value))
                }
              />
            </label>

            <div className="wallpaper-control-group wallpaper-recent-group">
              <span className="wallpaper-control-label">
                {t("prefs.wallpaper.recent")}
              </span>
              <div className="wallpaper-recent-list">
                {prefs.recent.slice(0, 2).map((asset) => {
                  const src = resolveMediaSrc(asset.path);
                  return (
                    <button
                      key={asset.id}
                      type="button"
                      className={
                        prefs.current?.id === asset.id ? "is-active" : ""
                      }
                      aria-label={asset.name}
                      title={asset.name}
                      onClick={() => controller.select(asset)}
                    >
                      {src ? <img src={src} alt="" /> : <ImageIcon size={17} />}
                    </button>
                  );
                })}
                <button
                  type="button"
                  className="wallpaper-recent-add"
                  aria-label={t("prefs.wallpaper.upload")}
                  title={t("prefs.wallpaper.upload")}
                  onClick={() => void chooseImage()}
                >
                  <Plus size={20} aria-hidden />
                </button>
              </div>
            </div>
          </div>
        </div>

        {error && !dialogOpen ? (
          <p className="wallpaper-error">{error}</p>
        ) : null}
        {applied ? (
          <p className="wallpaper-applied" role="status">
            <Check size={14} /> {t("prefs.wallpaper.applied")}
          </p>
        ) : null}
      </section>
      {dialog}
    </>
  );
}
