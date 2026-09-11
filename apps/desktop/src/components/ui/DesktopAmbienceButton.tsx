import { useId, useRef, useState } from "react";
import {
  Image,
  Palette,
  PawPrint,
  RotateCcw,
  Settings2,
  X,
  Check,
} from "lucide-react";
import DynamicPaletteButton from "./DynamicPaletteButton";
import { PopoverSurface } from "./Overlay";
import { SegmentedTabs } from "./SegmentedTabs";
import {
  useDesktopAmbience,
  type AmbienceProps,
} from "../../hooks/app/useDesktopAmbience";
import {
  ambienceWallpaperChoices,
  materializeWallpaper,
} from "../../lib/ui/desktopAmbience";
import { groupPetScenes } from "../../lib/ui/petScene";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import { useI18n } from "../../i18n/LocaleContext";
import "../../styles/features/desktop-ambience.css";

export default function DesktopAmbienceButton(props: AmbienceProps) {
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState("scenes");
  const [linked, setLinked] = useState(false);
  const [name, setName] = useState("");
  const [saving, setSaving] = useState(false);
  const [primary, setPrimary] = useState("#4f6ef7");
  const [secondary, setSecondary] = useState("#22b8a7");
  const anchor = useRef<HTMLButtonElement>(null);
  const id = useId();
  const { locale } = useI18n();
  const tr = (zh: string, en: string) => (locale.startsWith("zh") ? zh : en);
  const state = useDesktopAmbience(props, open);
  const current = materializeWallpaper(
    props.wallpaper.prefs,
    props.activeStyle.style,
    props.theme,
  );
  const hasWallpaper = current.mode === "wallpaper" && Boolean(current.current);
  const scene = state.scenes.find((s) => s.id === state.pet.activeSceneId);
  const adjusted = Boolean(scene && props.activeStyle.style?.id !== scene.id);
  const palette =
    current.adaptiveColor && hasWallpaper
      ? "wallpaper"
      : props.colors.style === "dynamic"
        ? "dynamic"
        : hasWallpaper || props.colors.style === "unified"
          ? "custom"
          : "default";
  const wallpapers = ambienceWallpaperChoices(
    props.wallpaper.prefs.recent,
    state.scenes,
  );
  const close = () => setOpen(false);
  const show = () => {
    setPrimary(
      current.customThemeColor ?? current.current?.accentColor ?? "#4f6ef7",
    );
    setSecondary(
      current.customHighlightColor ??
        current.current?.secondaryColor ??
        "#22b8a7",
    );
    setOpen(!open);
  };
  const manage = () => {
    close();
    props.onManage(tab === "scenes" ? "scenes" : "wallpapers");
  };
  return (
    <>
      <DynamicPaletteButton
        label={tr("桌面氛围", "Desktop ambience")}
        buttonRef={anchor}
        expanded={open}
        controls={id}
        animate={false}
        onReshuffle={show}
      />
      <PopoverSurface
        id={id}
        open={open}
        onClose={close}
        anchorRef={anchor}
        placement="above"
        align="end"
        minWidth={360}
        maxWidth={400}
        maxHeightCap={620}
        maxHeightRatio={0.86}
        sizeKey={`${tab}:${state.scenes.length}:${saving}:${state.error}`}
        trapFocus
        aria-label={tr("桌面氛围", "Desktop ambience")}
        className="desktop-ambience"
      >
        <header className="ambience-header">
          <div>
            <strong>{tr("桌面氛围", "Desktop ambience")}</strong>
            <p>{tr("给此刻，换一种心情", "A little change of scenery")}</p>
          </div>
          <button
            type="button"
            className="ambience-icon"
            onClick={close}
            aria-label={tr("关闭", "Close")}
          >
            <X size={18} />
          </button>
        </header>
        <div className="ambience-current" aria-live="polite">
          <span>
            {state.pet.displayName ?? tr("未选择宠物", "No pet selected")} ·{" "}
            {hasWallpaper
              ? current.current?.name
              : tr("纯色背景", "Color background")}
          </span>
          <small>
            {palette === "wallpaper"
              ? tr("跟随壁纸配色", "Wallpaper colors")
              : palette === "dynamic"
                ? tr("灵动配色", "Dynamic colors")
                : palette === "custom"
                  ? tr("自定义配色", "Custom colors")
                  : tr("默认配色", "Default colors")}
            {adjusted ? tr(" · 已调整", " · Modified") : ""}
          </small>
        </div>
        <SegmentedTabs
          aria-label={tr("氛围分类", "Ambience category")}
          value={tab}
          onValueChange={setTab}
          size="sm"
          items={[
            {
              value: "scenes",
              label: tr("场景", "Scenes"),
              icon: <PawPrint size={15} />,
              panelId: `${id}-scenes`,
            },
            {
              value: "wallpapers",
              label: tr("壁纸", "Wallpapers"),
              icon: <Image size={15} />,
              panelId: `${id}-wallpapers`,
            },
            {
              value: "palette",
              label: tr("配色", "Colors"),
              icon: <Palette size={15} />,
              panelId: `${id}-palette`,
            },
          ]}
        />
        <div
          className="ambience-content"
          role="tabpanel"
          id={`${id}-${tab}`}
          aria-label={
            tab === "scenes"
              ? tr("场景", "Scenes")
              : tab === "wallpapers"
                ? tr("壁纸", "Wallpapers")
                : tr("配色", "Colors")
          }
          aria-busy={state.busy}
        >
          {tab === "scenes" && (
            <>
              <p className="ambience-help">
                {tr(
                  "一起切换宠物与壁纸。不会重新生成素材。",
                  "Switch the pet and wallpaper together. No generation required.",
                )}
              </p>
              {groupPetScenes(state.scenes).map((group) => (
                <section key={group.scenes[0]?.pet.petId}>
                  <h4>{group.name ?? tr("我的宠物", "My pet")}</h4>
                  <div className="ambience-grid">
                    {group.scenes.map((item) => (
                      <button
                        type="button"
                        key={item.id}
                        disabled={state.busy || !item.wallpaperPath}
                        aria-pressed={scene?.id === item.id && !adjusted}
                        className="ambience-tile"
                        onClick={() => void state.selectScene(item.id)}
                      >
                        {item.wallpaperPath ? (
                          <img
                            src={
                              resolveMediaSrc(item.wallpaperPath) ?? undefined
                            }
                            alt=""
                            loading="lazy"
                          />
                        ) : (
                          <div className="ambience-no-image">
                            <Image size={24} />
                          </div>
                        )}
                        <span>
                          {item.name}
                          {scene?.id === item.id && !adjusted && (
                            <Check size={14} />
                          )}
                        </span>
                        {!item.wallpaperPath && (
                          <small>{tr("尚无壁纸", "No wallpaper yet")}</small>
                        )}
                      </button>
                    ))}
                  </div>
                </section>
              ))}
              {!state.scenes.length && (
                <p className="ambience-empty">
                  {state.busy
                    ? tr("正在读取场景…", "Loading scenes…")
                    : tr(
                        "还没有场景，去管理中心为宠物添加一个家。",
                        "No scenes yet. Add one in the pet library.",
                      )}
                </p>
              )}
            </>
          )}
          {tab === "wallpapers" && (
            <>
              <p className="ambience-help">
                {tr(
                  "默认只换背景，保留当前宠物。",
                  "Changes only the background by default; your pet stays.",
                )}
              </p>
              <label className="ambience-check">
                <input
                  type="checkbox"
                  checked={linked}
                  onChange={(e) => setLinked(e.target.checked)}
                />
                {tr(
                  "绑定场景的壁纸，同时切换宠物",
                  "For scene wallpapers, also switch the pet",
                )}
              </label>
              <div className="ambience-grid">
                {wallpapers.map((asset) => (
                  <button
                    type="button"
                    key={asset.path}
                    className="ambience-tile"
                    aria-pressed={
                      hasWallpaper && current.current?.path === asset.path
                    }
                    disabled={state.busy}
                    onClick={() => void state.selectWallpaper(asset, linked)}
                  >
                    <img
                      src={resolveMediaSrc(asset.path) ?? undefined}
                      alt=""
                      loading="lazy"
                    />
                    <span>{asset.name}</span>
                    <small>
                      {state.scenes.some((s) => s.wallpaperPath === asset.path)
                        ? tr("宠物场景", "Pet scene")
                        : asset.source === "upload"
                          ? tr("我的上传", "Uploaded")
                          : asset.source === "system"
                            ? tr("系统壁纸", "System")
                            : tr("AI 壁纸", "AI wallpaper")}
                    </small>
                  </button>
                ))}
              </div>
              {!wallpapers.length && (
                <p className="ambience-empty">
                  {tr(
                    "暂无壁纸，可在外观设置中上传或生成。",
                    "Upload or generate wallpapers in Appearance settings.",
                  )}
                </p>
              )}
              <div className="ambience-actions">
                <button
                  type="button"
                  disabled={state.busy}
                  onClick={() => void state.followSystem()}
                  aria-pressed={current.followSystemWallpaper}
                >
                  {tr("跟随系统壁纸", "Follow system wallpaper")}
                </button>
                <button
                  type="button"
                  disabled={state.busy || !hasWallpaper}
                  onClick={() => void state.clearWallpaper()}
                >
                  {tr("使用纯色背景", "Use color background")}
                </button>
              </div>
            </>
          )}
          {tab === "palette" && (
            <>
              <p className="ambience-help">
                {tr(
                  "只改颜色，保留壁纸和宠物。",
                  "Change colors without changing the wallpaper or pet.",
                )}
              </p>
              <div className="ambience-palette-options">
                <button
                  type="button"
                  aria-pressed={palette === "wallpaper"}
                  disabled={state.busy || !hasWallpaper}
                  onClick={() => void state.setPalette("wallpaper")}
                >
                  <Image size={18} />
                  <span>
                    {tr("跟随壁纸", "From wallpaper")}
                    <small>
                      {tr(
                        "自动取色，适配亮暗主题",
                        "Automatic colors for light and dark themes",
                      )}
                    </small>
                  </span>
                </button>
                <button
                  type="button"
                  aria-pressed={palette === "dynamic"}
                  disabled={state.busy}
                  onClick={() => void state.setPalette("dynamic")}
                >
                  <Palette size={18} />
                  <span>
                    {tr("灵动配色", "Dynamic palette")}
                    <small>
                      {tr(
                        "本地换一组颜色，不调用 AI",
                        "A fresh local palette, no AI request",
                      )}
                    </small>
                  </span>
                </button>
              </div>
              <form
                className="ambience-custom"
                onSubmit={(e) => {
                  e.preventDefault();
                  void state.setPalette("custom", primary, secondary);
                }}
              >
                <label>
                  {tr("主题色", "Primary")}
                  <input
                    type="color"
                    aria-label={tr("主题色", "Primary color")}
                    value={primary}
                    onChange={(e) => setPrimary(e.target.value)}
                  />
                </label>
                <label>
                  {tr("点缀色", "Accent")}
                  <input
                    type="color"
                    aria-label={tr("点缀色", "Accent color")}
                    value={secondary}
                    onChange={(e) => setSecondary(e.target.value)}
                  />
                </label>
                <button type="submit" disabled={state.busy}>
                  {tr("应用自定义", "Apply colors")}
                </button>
              </form>
              {adjusted && scene && (
                <div className="ambience-actions">
                  <button
                    type="button"
                    disabled={state.busy}
                    onClick={() => void state.selectScene(scene.id)}
                  >
                    {tr("还原场景", "Restore scene")}
                  </button>
                </div>
              )}
              {hasWallpaper && state.pet.activePetId && (
                <button
                  type="button"
                  className="ambience-save"
                  onClick={() => setSaving(!saving)}
                >
                  {tr("另存为场景", "Save as a new scene")}
                </button>
              )}
              {saving && (
                <form
                  className="ambience-save-form"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void state.saveAs(name).then((ok) => {
                      if (ok) {
                        setSaving(false);
                        setName("");
                      }
                    });
                  }}
                >
                  <input
                    aria-label={tr("新场景名称", "New scene name")}
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    maxLength={80}
                    placeholder={tr(
                      "为当前组合起个名字",
                      "Name this combination",
                    )}
                  />
                  <button type="submit" disabled={state.busy || !name.trim()}>
                    {tr("保存", "Save")}
                  </button>
                </form>
              )}
            </>
          )}
        </div>
        {state.error && (
          <p className="ambience-error" role="alert">
            {state.error}
          </p>
        )}
        <footer className="ambience-footer">
          <button
            type="button"
            disabled={!state.canUndo || state.busy}
            onClick={() => void state.undoLast()}
          >
            <RotateCcw size={14} />
            {tr("撤销上一步", "Undo last change")}
          </button>
          <button type="button" onClick={manage}>
            <Settings2 size={14} />
            {tr("管理场景与壁纸", "Manage")}
          </button>
        </footer>
      </PopoverSurface>
    </>
  );
}
