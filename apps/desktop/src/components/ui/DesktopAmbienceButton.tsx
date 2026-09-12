import { useEffect, useId, useRef, useState, type CSSProperties } from "react";
import {
  Image,
  Palette,
  PawPrint,
  Layers,
  RotateCcw,
  Settings2,
  X,
  Check,
  Star,
  Shuffle,
  LockKeyhole,
  MoreHorizontal,
  Upload,
} from "lucide-react";
import DynamicPaletteButton from "./DynamicPaletteButton";
import { PopoverSurface } from "./Overlay";
import { SegmentedTabs } from "./SegmentedTabs";
import DesktopPetVisibilityButton from "../desktop-pet/DesktopPetVisibilityButton";
import DesktopPetCanvas from "../desktop-pet/DesktopPetCanvas";
import {
  useDesktopAmbience,
  type AmbienceProps,
} from "../../hooks/app/useDesktopAmbience";
import {
  ambienceWallpaperChoices,
  materializeWallpaper,
} from "../../lib/ui/desktopAmbience";
import {
  SHELL_GRADIENT_PRESETS,
  gradientFromPreset,
  gradientSwatchBackground,
  type ShellColorStyle,
} from "../../lib/ui/shellGradient";
import { createDynamicSeed } from "../../lib/ui/dynamicGradient";
import type { PetScene } from "../../lib/ui/petScene";
import type { ShuffleScope } from "../../lib/ui/ambienceShuffle";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import { useI18n } from "../../i18n/LocaleContext";

function PetAvatar({ pet }: { pet: PetScene["pet"] }) {
  const src = resolveMediaSrc(pet.petPath);
  if (!src) return <PawPrint size={22} aria-hidden />;
  return [2, 3].includes(pet.spriteVersionNumber ?? 0) ? (
    <DesktopPetCanvas
      src={src}
      state="idle"
      reducedMotion
      className="ambience-pet-avatar"
      label={pet.displayName ?? "Pet"}
    />
  ) : (
    <img className="ambience-pet-avatar" src={src} alt="" />
  );
}

export default function DesktopAmbienceButton(props: AmbienceProps) {
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState("background");
  const [background, setBackground] = useState("color");
  const [browsedPet, setBrowsedPet] = useState<string | null>(null);
  const [linked, setLinked] = useState(false);
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [saving, setSaving] = useState(false);
  const [name, setName] = useState("");
  const anchor = useRef<HTMLButtonElement>(null);
  const id = useId();
  const { locale, t } = useI18n();
  const tr = (zh: string, en: string) => (locale.startsWith("zh") ? zh : en);
  const state = useDesktopAmbience(props, open);
  const appearance = state.appearance;
  const current = materializeWallpaper(
    props.wallpaper.prefs,
    props.activeStyle.style,
    props.theme,
  );
  const hasWallpaper = current.mode === "wallpaper" && Boolean(current.current);
  useEffect(() => {
    if (open) setBackground(hasWallpaper ? "wallpaper" : "color");
  }, [hasWallpaper, open]);
  const scene = state.scenes.find((s) => s.id === state.pet.activeSceneId);
  const adjusted = Boolean(scene && props.activeStyle.style?.id !== scene.id);
  const pets = new Map<string, PetScene["pet"]>();
  for (const pet of state.pet.pets ?? []) pets.set(pet.id, pet.identity);
  for (const item of state.scenes)
    if (!pets.has(item.pet.petId)) pets.set(item.pet.petId, item.pet);
  const selectedPet =
    browsedPet && pets.has(browsedPet)
      ? browsedPet
      : (state.pet.activePetId ?? pets.keys().next().value);
  const petScenes = state.scenes.filter((s) => s.pet.petId === selectedPet);
  const wallpapers = ambienceWallpaperChoices(
    [
      ...(current.current ? [current.current] : []),
      ...props.wallpaper.prefs.recent,
    ],
    [],
  );
  const favoritePaths = new Set(
    state.shufflePrefs.favorites.map((asset) => asset.path),
  );
  const visibleWallpapers = favoritesOnly
    ? state.shufflePrefs.favorites
    : wallpapers;
  const scope: ShuffleScope | null =
    tab === "pets" && selectedPet === state.pet.activePetId
      ? "pet-scenes"
      : tab === "background" && background === "wallpaper"
        ? "favorites"
        : tab === "background" && props.colors.style === "dynamic"
          ? "palette"
          : null;
  const availability = scope ? state.rotationChoices(scope) : null;
  const shuffleLabel =
    scope === "palette"
      ? tr("换组颜色", "New colors")
      : scope === "favorites"
        ? tr("随机收藏", "Shuffle favorites")
        : tr("换个场景", "Another scene");
  const shuffleHint =
    availability?.reason === "locked"
      ? tr("已锁定随机换景", "Random changes locked")
      : availability?.reason === "no-favorite"
        ? tr("先收藏另一张壁纸", "Favorite another wallpaper first")
        : availability?.reason === "no-scene"
          ? tr("当前宠物暂无其他场景", "No other scene for the current pet")
          : availability?.reason === "no-pet"
            ? tr("先选择一只宠物", "Select a pet first")
            : scope === "pet-scenes"
              ? tr(
                  "只换当前桌宠的场景",
                  "Only the current desktop pet's scenes",
                )
              : "";
  const close = () => {
    state.finishAppearanceEdit();
    setOpen(false);
  };
  const manage = () => {
    close();
    props.onManage(tab === "pets" ? "scenes" : "wallpapers");
  };
  const show = () => {
    if (open) return close();
    setBackground(hasWallpaper ? "wallpaper" : "color");
    setBrowsedPet(null);
    setOpen(true);
  };
  const chooseColorStyle = (style: ShellColorStyle) =>
    void state.setAmbientColors({
      ...props.colors,
      style,
      dynamicSeed:
        style === "dynamic" ? createDynamicSeed() : props.colors.dynamicSeed,
    });
  const strength =
    appearance.material === "soft"
      ? appearance.softFrostIntensity
      : appearance.glassIntensity;
  const tabLabel =
    tab === "material"
      ? tr("材质", "Material")
      : tab === "pets"
        ? tr("宠物", "Pets")
        : tr("背景", "Background");

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
        minWidth={380}
        maxWidth={400}
        maxHeightCap={620}
        maxHeightRatio={0.75}
        sizeKey={[
          tab,
          background,
          selectedPet,
          state.scenes.length,
          saving,
          state.error,
          favoritesOnly,
        ].join(":")}
        trapFocus
        aria-label={tr("桌面氛围", "Desktop ambience")}
        className="desktop-ambience"
      >
        <header className="ambience-header">
          <strong>{tr("桌面氛围", "Desktop ambience")}</strong>
          <button
            type="button"
            className="ambience-icon"
            onClick={close}
            aria-label={tr("关闭", "Close")}
          >
            <X size={18} />
          </button>
        </header>
        <div
          className="ambience-current"
          aria-live="polite"
          title={hasWallpaper ? current.current?.name : undefined}
        >
          {appearance.material === "soft"
            ? tr("柔塑", "Soft")
            : tr("玻璃", "Glass")}{" "}
          ·{" "}
          {hasWallpaper
            ? tr("图片壁纸", "Wallpaper")
            : tr("氛围配色", "Ambient colors")}{" "}
          · {state.pet.displayName ?? tr("无桌宠", "No pet")}
        </div>
        <SegmentedTabs
          aria-label={tr("氛围分类", "Ambience category")}
          value={tab}
          onValueChange={(value) => {
            state.finishAppearanceEdit();
            setTab(value);
          }}
          size="sm"
          items={[
            {
              value: "material",
              label: tr("材质", "Material"),
              icon: <Layers size={15} />,
              panelId: id + "-material",
            },
            {
              value: "background",
              label: tr("背景", "Background"),
              icon: <Image size={15} />,
              panelId: id + "-background",
            },
            {
              value: "pets",
              label: tr("宠物", "Pets"),
              icon: <PawPrint size={15} />,
              panelId: id + "-pets",
            },
          ]}
        />
        <div
          className="ambience-content"
          role="tabpanel"
          id={id + "-" + tab}
          aria-label={tabLabel}
          aria-busy={state.busy}
        >
          {tab === "material" && (
            <div className="ambience-material">
              <fieldset disabled={state.busy} className="ambience-choice">
                <legend>{tr("界面材质", "Interface material")}</legend>
                <div>
                  {(["glass", "soft"] as const).map((value) => (
                    <button
                      key={value}
                      type="button"
                      aria-pressed={appearance.material === value}
                      onClick={() =>
                        state.changeAppearance({ material: value })
                      }
                    >
                      {value === "soft"
                        ? tr("柔塑 Soft", "Soft")
                        : tr("玻璃", "Glass")}
                    </button>
                  ))}
                </div>
              </fieldset>
              <fieldset disabled={state.busy} className="ambience-choice">
                <legend>{tr("明暗模式", "Appearance")}</legend>
                <div>
                  {(["light", "auto", "dark"] as const).map((value) => (
                    <button
                      key={value}
                      type="button"
                      aria-pressed={appearance.mode === value}
                      onClick={() => state.changeAppearance({ mode: value })}
                    >
                      {value === "light"
                        ? tr("浅色", "Light")
                        : value === "auto"
                          ? tr("系统", "System")
                          : tr("深色", "Dark")}
                    </button>
                  ))}
                </div>
              </fieldset>
              <label className="ambience-strength">
                <span>
                  {appearance.material === "soft"
                    ? tr("毛玻璃强度", "Frost strength")
                    : tr("玻璃强度", "Glass strength")}
                  <output>{strength}%</output>
                </span>
                <input
                  type="range"
                  min={0}
                  max={100}
                  step={1}
                  value={strength}
                  disabled={state.busy}
                  onPointerDown={state.beginAppearanceEdit}
                  onKeyDown={(event) => {
                    if (
                      !event.repeat &&
                      [
                        "ArrowLeft",
                        "ArrowRight",
                        "ArrowUp",
                        "ArrowDown",
                        "Home",
                        "End",
                        "PageUp",
                        "PageDown",
                      ].includes(event.key)
                    )
                      state.beginAppearanceEdit();
                  }}
                  onPointerUp={state.finishAppearanceEdit}
                  onPointerCancel={state.finishAppearanceEdit}
                  onKeyUp={state.finishAppearanceEdit}
                  onBlur={state.finishAppearanceEdit}
                  onChange={(event) =>
                    state.changeAppearance(
                      appearance.material === "soft"
                        ? { softFrostIntensity: Number(event.target.value) }
                        : { glassIntensity: Number(event.target.value) },
                    )
                  }
                />
              </label>
              <p className="ambience-help">
                {tr(
                  "只调材质，不改变背景和宠物",
                  "Only changes the material, not the background or pet",
                )}
              </p>
            </div>
          )}
          {tab === "background" && (
            <>
              <div
                className="ambience-choice ambience-background-mode"
                role="group"
                aria-label={tr("背景类型", "Background type")}
              >
                <div>
                  <button
                    type="button"
                    aria-pressed={background === "color"}
                    disabled={state.busy}
                    onClick={() => {
                      if (hasWallpaper)
                        void state.clearWallpaper().then((ok) => {
                          if (ok) setBackground("color");
                        });
                      else setBackground("color");
                    }}
                  >
                    <Palette size={15} />
                    {tr("氛围配色", "Ambient colors")}
                  </button>
                  <button
                    type="button"
                    aria-pressed={background === "wallpaper"}
                    disabled={state.busy}
                    onClick={() => setBackground("wallpaper")}
                  >
                    <Image size={15} />
                    {tr("图片壁纸", "Wallpaper")}
                  </button>
                </div>
              </div>
              {background === "color" ? (
                <>
                  <fieldset className="ambience-choice" disabled={state.busy}>
                    <legend>{tr("色彩策略", "Color strategy")}</legend>
                    <div>
                      {(["unified", "dynamic", "colorful"] as const).map(
                        (value) => (
                          <button
                            key={value}
                            type="button"
                            aria-pressed={props.colors.style === value}
                            onClick={() => chooseColorStyle(value)}
                          >
                            {t(
                              ("prefs.colorStyle." +
                                value) as "prefs.colorStyle.unified",
                            )}
                          </button>
                        ),
                      )}
                    </div>
                  </fieldset>
                  {props.colors.style === "unified" && (
                    <div
                      className="ambience-swatches"
                      role="group"
                      aria-label={tr("氛围色预设", "Ambient color presets")}
                    >
                      {SHELL_GRADIENT_PRESETS.map((preset) => (
                        <button
                          key={preset.id}
                          type="button"
                          disabled={state.busy}
                          aria-label={t(preset.labelKey)}
                          title={t(preset.labelKey)}
                          aria-pressed={props.colors.gradient.id === preset.id}
                          style={
                            {
                              "--ambience-swatch": gradientSwatchBackground(
                                gradientFromPreset(preset.id),
                              ),
                            } as CSSProperties
                          }
                          onClick={() =>
                            void state.setAmbientColors({
                              ...props.colors,
                              style: "unified",
                              gradient: gradientFromPreset(preset.id),
                            })
                          }
                        >
                          {props.colors.gradient.id === preset.id && (
                            <Check size={14} />
                          )}
                        </button>
                      ))}
                    </div>
                  )}
                  <p className="ambience-help">
                    {props.colors.style === "colorful"
                      ? tr("按功能分配颜色", "Colors follow each workspace")
                      : tr(
                          "只换背景配色，保留当前宠物",
                          "Changes background colors; your pet stays",
                        )}
                  </p>
                  <div className="ambience-actions">
                    <button type="button" onClick={manage}>
                      {tr("自定义渐变…", "Custom gradient…")}
                    </button>
                  </div>
                </>
              ) : (
                <>
                  <div
                    className="ambience-choice ambience-wallpaper-filter"
                    role="group"
                    aria-label={tr("壁纸筛选", "Wallpaper filter")}
                  >
                    <div>
                      <button
                        type="button"
                        aria-pressed={!favoritesOnly}
                        onClick={() => setFavoritesOnly(false)}
                      >
                        {tr("最近", "Recent")}
                      </button>
                      <button
                        type="button"
                        aria-pressed={favoritesOnly}
                        onClick={() => setFavoritesOnly(true)}
                      >
                        {tr("收藏", "Favorites")}
                      </button>
                    </div>
                  </div>
                  <div className="ambience-grid">
                    {visibleWallpapers.map((asset) => (
                      <div key={asset.path} className="ambience-wallpaper-card">
                        <button
                          type="button"
                          className="ambience-tile"
                          aria-pressed={
                            hasWallpaper && current.current?.path === asset.path
                          }
                          disabled={state.busy}
                          onClick={() =>
                            void state.selectWallpaper(asset, linked)
                          }
                        >
                          <img
                            src={resolveMediaSrc(asset.path) ?? undefined}
                            alt=""
                            loading="lazy"
                          />
                          <span>{asset.name}</span>
                        </button>
                        <button
                          type="button"
                          className="ambience-star"
                          disabled={state.busy}
                          aria-label={
                            (favoritePaths.has(asset.path)
                              ? tr("取消收藏 ", "Unfavorite ")
                              : tr("收藏 ", "Favorite ")) + asset.name
                          }
                          aria-pressed={favoritePaths.has(asset.path)}
                          onClick={() => state.toggleFavorite(asset)}
                        >
                          <Star
                            size={14}
                            fill={
                              favoritePaths.has(asset.path)
                                ? "currentColor"
                                : "none"
                            }
                          />
                        </button>
                      </div>
                    ))}
                  </div>
                  {!visibleWallpapers.length && (
                    <p className="ambience-empty">
                      {favoritesOnly
                        ? tr(
                            "点击壁纸星标添加收藏",
                            "Star a wallpaper to add favorites",
                          )
                        : tr("还没有图片壁纸", "No wallpapers yet")}
                    </p>
                  )}
                  <div className="ambience-actions">
                    <button type="button" onClick={manage}>
                      <Upload size={14} />
                      {tr("上传或生成…", "Upload or generate…")}
                    </button>
                  </div>
                  <label className="ambience-check">
                    <input
                      type="checkbox"
                      disabled={state.busy}
                      checked={current.followSystemWallpaper}
                      onChange={(event) =>
                        void (event.target.checked
                          ? state.followSystem()
                          : state.stopFollowingSystem())
                      }
                    />
                    {tr("跟随系统壁纸", "Follow system wallpaper")}
                  </label>
                  <label className="ambience-check">
                    <input
                      type="checkbox"
                      disabled={state.busy || !hasWallpaper}
                      checked={current.adaptiveColor}
                      onChange={(event) =>
                        void state.setPalette(
                          event.target.checked ? "wallpaper" : "custom",
                          current.customThemeColor ??
                            current.current?.accentColor ??
                            "#4f6ef7",
                          current.customHighlightColor ??
                            current.current?.secondaryColor ??
                            "#22b8a7",
                        )
                      }
                    />
                    {tr("从壁纸自动取色", "Use wallpaper colors")}
                  </label>
                  <label className="ambience-check">
                    <input
                      type="checkbox"
                      checked={linked}
                      onChange={(event) => setLinked(event.target.checked)}
                    />
                    {tr(
                      "关联壁纸同时切换宠物",
                      "Also switch the pet for linked wallpapers",
                    )}
                  </label>
                  <p className="ambience-help">
                    {linked
                      ? tr(
                          "选择关联壁纸将切换对应宠物",
                          "Linked wallpapers also switch their pet",
                        )
                      : tr(
                          "只换图片，保留当前宠物",
                          "Only changes the image; your pet stays",
                        )}
                  </p>
                </>
              )}
            </>
          )}
          {tab === "pets" && (
            <>
              <div className="ambience-pet-status sidebar-footer-actions">
                <span>{tr("桌面显示", "Desktop visibility")}</span>
                <DesktopPetVisibilityButton onError={state.reportError} />
              </div>
              <div
                className="ambience-pets"
                role="group"
                aria-label={tr("浏览宠物", "Browse pets")}
              >
                {[...pets].map(([petId, pet]) => (
                  <button
                    type="button"
                    key={petId}
                    aria-label={pet.displayName ?? tr("宠物", "Pet")}
                    aria-pressed={selectedPet === petId}
                    onClick={() => setBrowsedPet(petId)}
                  >
                    <PetAvatar pet={pet} />
                    <span>{pet.displayName ?? tr("宠物", "Pet")}</span>
                    {petId === state.pet.activePetId && (
                      <Check size={12} aria-hidden />
                    )}
                  </button>
                ))}
              </div>
              <p className="ambience-help">
                {tr(
                  "浏览不切换；选场景会应用宠物和背景",
                  "Browsing keeps your desktop; a scene applies its pet and background",
                )}
              </p>
              <div className="ambience-grid">
                {petScenes.map((item) => (
                  <button
                    type="button"
                    key={item.id}
                    className="ambience-tile ambience-scene-tile"
                    disabled={state.busy || !item.wallpaperPath}
                    aria-pressed={scene?.id === item.id && !adjusted}
                    onClick={() => void state.selectScene(item.id)}
                  >
                    {item.wallpaperPath ? (
                      <img
                        src={resolveMediaSrc(item.wallpaperPath) ?? undefined}
                        alt=""
                        loading="lazy"
                      />
                    ) : (
                      <div className="ambience-no-image">
                        <Image size={24} />
                      </div>
                    )}
                    <span className="ambience-scene-pet" aria-hidden>
                      <PetAvatar pet={item.pet} />
                    </span>
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
              {!petScenes.length && (
                <p className="ambience-empty">
                  {state.busy
                    ? tr("正在读取场景…", "Loading scenes…")
                    : tr(
                        "暂无场景，前往设置创建",
                        "No scenes yet. Create one in Settings",
                      )}
                </p>
              )}
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
            </>
          )}
          {scope && (
            <div className="ambience-context-action">
              <button
                type="button"
                disabled={state.busy || !availability?.choices.length}
                onClick={() => void state.shuffle(scope)}
                aria-describedby={id + "-shuffle-hint"}
              >
                <Shuffle size={14} />
                {shuffleLabel}
              </button>
              <details className="ambience-more">
                <summary aria-label={tr("随机选项", "Random options")}>
                  <MoreHorizontal size={18} />
                </summary>
                <label className="ambience-check">
                  <input
                    type="checkbox"
                    checked={state.shufflePrefs.backgroundLocked}
                    disabled={state.busy}
                    onChange={(e) =>
                      state.setBackgroundLocked(e.target.checked)
                    }
                  />
                  <LockKeyhole size={12} />
                  {tr("锁定随机换景", "Lock random backgrounds")}
                </label>
                <small>
                  {tr(
                    "手动选择和换色不受影响",
                    "Manual choices and colors are unaffected",
                  )}
                </small>
              </details>
              {shuffleHint && (
                <small id={id + "-shuffle-hint"}>{shuffleHint}</small>
              )}
            </div>
          )}
          {tab === "pets" && hasWallpaper && state.pet.activePetId && (
            <details
              className="ambience-save-options"
              onToggle={(event) => setSaving(event.currentTarget.open)}
            >
              <summary>
                {tr(
                  "另存当前组合为场景",
                  "Save current combination as a scene",
                )}
              </summary>
              {saving && (
                <form
                  className="ambience-save-form"
                  onSubmit={(event) => {
                    event.preventDefault();
                    void state.saveAs(name).then((ok) => {
                      if (ok) setName("");
                    });
                  }}
                >
                  <input
                    aria-label={tr("新场景名称", "New scene name")}
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    maxLength={80}
                    placeholder={tr("场景名称", "Scene name")}
                  />
                  <button type="submit" disabled={state.busy || !name.trim()}>
                    {tr("保存", "Save")}
                  </button>
                </form>
              )}
            </details>
          )}
          {state.error && (
            <p className="ambience-error" role="alert">
              {state.error}
            </p>
          )}
        </div>
        <footer className="ambience-footer">
          <button
            type="button"
            disabled={!state.canUndo || state.busy}
            onClick={() => void state.undoLast()}
          >
            <RotateCcw size={14} />
            {tr("撤销", "Undo")}
          </button>
          <button type="button" onClick={manage}>
            <Settings2 size={14} />
            {tr("更多设置", "More settings")}
          </button>
        </footer>
      </PopoverSurface>
    </>
  );
}
