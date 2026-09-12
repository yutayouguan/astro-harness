import {
  FolderInput,
  ImagePlus,
  Loader2,
  Upload,
} from "lucide-react";
import { AIActionIcon } from "../icons/AIActionIcon";
import { invoke } from "@tauri-apps/api/core";
import { useId, useRef, useState } from "react";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import type { DesktopPetState } from "../../lib/ui/desktopPetState";
import PetSettingSwitch from "./PetSettingSwitch";
const COPY = {
  zh: {
    eyebrow: "ASTRO DESKTOP COMPANION",
    title: "把熟悉的它，带到桌面上",
    subtitle:
      "用宠物照片生成专属静态桌宠。",
    source: "宠物照片",
    sourceHint: "建议使用正面、光线均匀、主体完整的照片",
    upload: "选择照片",
    replace: "更换照片",
    create: "生成静态形象",
    creating: "正在生成桌宠…",
    style: "希望它呈现什么感觉？",
    stylePlaceholder: "例如：圆润 Q 版、温柔安静、保留蓝色项圈",
    show: "显示桌宠",
    showHint: "在桌面上打开独立透明悬浮窗口",
    pin: "始终置顶",
    pinHint: "让桌宠保持在其他窗口上方",
    size: "桌宠大小",
    generated: "当前桌宠",
    generatedHint: "拖动可移动；右键可打开主窗口、进入设置或隐藏桌宠",
    empty: "上传照片后即可生成",
    provider: "生成模型",
    importAnimated: "导入动画桌宠",
    importAnimatedHint: "选择 Astro 动画宠物包中的 pet.json，也支持 v2 图集",
    animatedBadge: "动画 v2",
    staticHint:
      "照片仅生成静态形象；动画可用内置奶糖或导入宠物包。",
    staticBadge: "静态图片",
  },
  en: {
    eyebrow: "ASTRO DESKTOP COMPANION",
    title: "Bring a familiar friend to your desktop",
    subtitle:
      "Create a static desktop pet from a photo.",
    source: "Pet photo",
    sourceHint: "Use a well-lit photo with the full subject clearly visible",
    upload: "Choose photo",
    replace: "Replace photo",
    create: "Create static portrait",
    creating: "Creating your pet…",
    style: "How should your companion feel?",
    stylePlaceholder:
      "For example: soft chibi style, calm expression, keep the blue collar",
    show: "Show desktop pet",
    showHint: "Open it in a separate transparent floating window",
    pin: "Always on top",
    pinHint: "Keep the companion above other windows",
    size: "Pet size",
    generated: "Current companion",
    generatedHint:
      "Drag to move; right-click to open Astro, settings, or hide the pet",
    empty: "Upload a photo to start creating",
    provider: "Generated with",
    importAnimated: "Import animated pet",
    importAnimatedHint:
      "Choose pet.json from an Astro animation package; v2 atlases are also supported",
    animatedBadge: "Animated v2",
    staticHint:
      "Photos produce static portraits. For animation, use Naitang or import a pet package.",
    staticBadge: "Static image",
  },
} as const;

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export default function PetCreatePanel({
  state,
  mutate,
  pending,
  locale,
}: {
  state: DesktopPetState;
  mutate: (
    command: string,
    args: Record<string, unknown>,
  ) => Promise<DesktopPetState>;
  pending: boolean;
  locale: "zh" | "en";
}) {
  const copy = COPY[locale],
    zh = locale === "zh";
  const [description, setDescription] = useState("");
  const [sceneName, setSceneName] = useState("");
  const [petName, setPetName] = useState("");
  const generationId = useRef<string | null>(null);
  const generationCancelled = useRef(false);
  const [cancelling, setCancelling] = useState(false);
  const [withWallpaper, setWithWallpaper] = useState(false);
  const [sceneDescription, setSceneDescription] = useState("");
  const [includePet, setIncludePet] = useState(false);
  const [notice, setNotice] = useState("");
  const [phase, setPhase] = useState<
    "upload" | "generate" | "wallpaper" | null
  >(null);
  const [error, setLocalError] = useState("");
  const busy = phase ?? (pending ? "load" : null);
  const sourceSrc = resolveMediaSrc(state.sourcePath) || "";
  const wallpaperOptionsId = useId();
  async function choosePhoto() {
    if (busy) return;
    setPhase("upload");
    setLocalError("");

    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        directory: false,
        title: copy.upload,
        filters: [
          { name: copy.source, extensions: ["png", "jpg", "jpeg", "webp"] },
        ],
      });
      if (typeof selected === "string") {
        await mutate("import_desktop_pet_photo", { sourcePath: selected });
      }
    } catch (cause) {
      setLocalError(errorMessage(cause));
    } finally {
      setPhase(null);
    }
  }

  async function generate() {
    if (!state.sourcePath || busy) return;
    setPhase("generate");
    setLocalError("");
    setNotice("");
    generationCancelled.current = false;
    generationId.current = crypto.randomUUID();
    try {
      const saved = await mutate("create_pet_scene", {
        requestId: generationId.current,
        petName: petName.trim() || (zh ? "我的宠物" : "My pet"),
        name: sceneName.trim() || (zh ? "我的宠物场景" : "My companion scene"),
        description: description.trim() || null,
        useCurrent: false,
      });
      setNotice(
        zh
          ? "桌宠已保存到宠物库，请预览后应用。"
          : "Pet saved to the library. Preview before applying.",
      );
      if (withWallpaper) {
        if (generationCancelled.current) return;
        generationId.current = crypto.randomUUID();
        setPhase("wallpaper");
        const sceneId = saved.scenes[saved.scenes.length - 1]?.id;
        if (!sceneId) throw new Error("Missing saved scene");
        await mutate("generate_pet_scene_wallpaper", {
          requestId: generationId.current,
          sceneId,
          description:
            sceneDescription.trim() ||
            (zh ? "温暖安静的森林小屋" : "A warm peaceful woodland home"),
          includePet,
        });
      }
    } catch (cause) {
      setLocalError(errorMessage(cause));
    } finally {
      generationId.current = null;
      setCancelling(false);
      setPhase(null);
    }
  }

  async function cancelGeneration() {
    generationCancelled.current = true;
    setCancelling(true);
    try {
      if (generationId.current)
        await invoke("cancel_pet_generation", {
          requestId: generationId.current,
        });
      setNotice(
        zh
          ? "已请求取消。已保存的桌宠保留，远端请求可能仍计费。"
          : "Cancellation requested. Saved pets are kept; submitted requests may still be billed.",
      );
    } catch (cause) {
      setLocalError(errorMessage(cause));
      setCancelling(false);
    }
  }

  async function importAnimatedPackage() {
    if (busy) return;
    setPhase("upload");
    setLocalError("");

    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        directory: false,
        title: copy.importAnimated,
        filters: [{ name: "pet.json", extensions: ["json"] }],
      });
      if (typeof selected === "string") {
        await mutate("import_desktop_pet_package", { manifestPath: selected });
        setNotice(
          zh
            ? "已导入宠物库，请预览后应用。"
            : "Imported to the library. Preview before applying.",
        );
      }
    } catch (cause) {
      setLocalError(errorMessage(cause));
    } finally {
      setPhase(null);
    }
  }

  return (
    <div className="pet-create-layout">
      <header className="pet-page-heading">
        <h3>{zh ? "创建专属伙伴" : "Create your companion"}</h3>
        <p>
          {zh
            ? "从照片生成静态形象，或导入准备好的动画宠物包。"
            : "Create a static portrait from a photo, or import a ready-made animated pet."}
        </p>
      </header>
      <section
        className="prefs-card desktop-pet-card pet-create-card"
        aria-label={zh ? "照片生成" : "Create from photo"}
      >
        <div className="pet-create-source">
          <header className="desktop-pet-card-head">
            <span className="desktop-pet-icon" aria-hidden>
              <ImagePlus size={19} />
            </span>
            <div>
              <h3>{copy.source}</h3>
              <p>{copy.sourceHint}</p>
            </div>
          </header>
          <button
            type="button"
            className="desktop-pet-photo-picker"
            data-has-photo={!!sourceSrc}
            aria-label={sourceSrc ? copy.replace : copy.upload}
            onClick={() => void choosePhoto()}
            disabled={busy != null}
          >
            {sourceSrc ? (
              <img src={sourceSrc} alt={copy.source} />
            ) : (
              <span className="pet-photo-empty">
                <Upload size={26} aria-hidden />
                <strong>{copy.upload}</strong>
                <small>PNG · JPG · WEBP</small>
              </span>
            )}
            {sourceSrc && (
              <span className="pet-photo-replace">
                <Upload size={15} />
                {copy.replace}
              </span>
            )}
          </button>
          <p className="pet-create-photo-hint">
            {zh
              ? "建议主体完整、面部清晰；选择照片不会立即发送。"
              : "Choose a clear, full-body photo. Selecting a file does not send it."}
          </p>
        </div>
        <div className="pet-create-fields">
          <header className="pet-create-fields-head">
            <h3>{zh ? "形象与场景" : "Portrait and scene"}</h3>
            <span className="pet-static-badge">
              {zh ? "静态形象" : "Static portrait"}
            </span>
          </header>
          <label className="desktop-pet-field">
            <span>
              {zh ? "宠物名字" : "Pet name"}{" "}
              <small>{zh ? "可选" : "Optional"}</small>
            </span>
            <input
              value={petName}
              maxLength={80}
              disabled={busy != null}
              placeholder={zh ? "例如：奶糖" : "For example: Mochi"}
              onChange={(e) => setPetName(e.currentTarget.value)}
            />
          </label>
          <label className="desktop-pet-field">
            <span>
              {zh ? "首个场景名称" : "First scene name"}{" "}
              <small>{zh ? "可选" : "Optional"}</small>
            </span>
            <input
              value={sceneName}
              maxLength={80}
              disabled={busy != null}
              placeholder={
                zh
                  ? "留空自动命名，之后可修改"
                  : "Auto-named if empty; editable later"
              }
              onChange={(e) => setSceneName(e.currentTarget.value)}
            />
          </label>
          <label className="desktop-pet-field pet-create-style">
            <span>
              {copy.style} <small>{zh ? "可选" : "Optional"}</small>
            </span>
            <textarea
              value={description}
              onChange={(e) => setDescription(e.currentTarget.value)}
              placeholder={copy.stylePlaceholder}
              maxLength={2000}
              disabled={busy != null}
            />
          </label>
          <div className="pet-wallpaper-block">
            <PetSettingSwitch
              label={zh ? "同时生成配套壁纸" : "Generate a matching wallpaper"}
              description={
                zh
                  ? "额外调用一次图片模型，仅用于 Astro 背景。"
                  : "One extra image request, for Astro's background only."
              }
              checked={withWallpaper}
              disabled={busy != null}
              onChange={setWithWallpaper}
              controls={wallpaperOptionsId}
            />
            <div
              id={wallpaperOptionsId}
              className="pet-wallpaper-options"
              hidden={!withWallpaper}
            >
              <label className="desktop-pet-field">
                <span>{zh ? "场景描述" : "Scene description"}</span>
                <textarea
                  value={sceneDescription}
                  maxLength={2000}
                  disabled={busy != null}
                  placeholder={
                    zh
                      ? "例如：温暖安静的森林小屋"
                      : "A warm, peaceful woodland home"
                  }
                  onChange={(e) => setSceneDescription(e.currentTarget.value)}
                />
              </label>
              <PetSettingSwitch
                label={zh ? "壁纸中包含宠物肖像" : "Include a pet portrait"}
                description={
                  zh
                    ? "默认只生成环境，避免与悬浮宠物重复。"
                    : "Environment only by default, avoiding a duplicate pet."
                }
                checked={includePet}
                disabled={busy != null}
                onChange={setIncludePet}
              />
            </div>
          </div>
        </div>
        <div className="pet-create-footer">
          <div className="pet-create-submit">
            <div>
              <strong>
                {!state.sourcePath
                  ? zh
                    ? "先选择一张宠物照片"
                    : "Choose a pet photo first"
                  : zh
                    ? "准备好后，生成你的伙伴"
                    : "Ready to create your companion"}
              </strong>
              <p>
                {zh
                  ? "静态形象先保存到宠物库，手动应用后替换桌面。"
                  : "Static portraits are saved to the library. Apply one to replace the desktop pet."}
              </p>
            </div>
            <button
              type="button"
              className="desktop-pet-generate"
              onClick={() => void generate()}
              disabled={!state.sourcePath || busy != null}
            >
              {phase === "generate" || phase === "wallpaper" ? (
                <Loader2 className="desktop-pet-spinner" size={17} />
              ) : (
                <AIActionIcon size={17} />
              )}
              {phase === "wallpaper"
                ? zh
                  ? "正在生成配套壁纸…"
                  : "Creating wallpaper…"
                : phase === "generate"
                  ? copy.creating
                  : copy.create}
            </button>
          </div>
          <p className="pet-create-privacy">
            {zh
              ? "仅在点击生成后，照片才会发送到所选图片服务商；本地副本与结果保存在本机。"
              : "Only generating sends your photo to the selected image provider. Local copies and results stay on this device."}
          </p>
          {(phase === "generate" || phase === "wallpaper") && (
            <div className="pet-generation-progress" role="status">
              <ol>
                <li aria-current={phase === "generate" ? "step" : undefined}>
                  {phase === "wallpaper" ? "✓ " : "1. "}
                  {zh ? "生成桌宠" : "Create pet"}
                </li>
                {withWallpaper && (
                  <li aria-current={phase === "wallpaper" ? "step" : undefined}>
                    2. {zh ? "生成配套壁纸" : "Create wallpaper"}
                  </li>
                )}
              </ol>
              <button
                type="button"
                disabled={cancelling}
                onClick={() => void cancelGeneration()}
              >
                {cancelling
                  ? zh
                    ? "取消中…"
                    : "Cancelling…"
                  : zh
                    ? "取消生成"
                    : "Cancel generation"}
              </button>
            </div>
          )}
        </div>
      </section>
      <section
        className="prefs-card pet-import-card"
        aria-label={copy.importAnimated}
      >
        <span className="desktop-pet-icon" aria-hidden>
          <FolderInput size={20} />
        </span>
        <div>
          <h3>{zh ? "已经有动画宠物？" : "Already have an animated pet?"}</h3>
          <p>
            {zh
              ? "选择宠物包中的 pet.json，无需上传照片或调用图片模型。"
              : "Choose pet.json from your pet package. No photo or image request needed."}
          </p>
        </div>
        <button
          type="button"
          className="desktop-pet-import-package"
          title={copy.importAnimatedHint}
          onClick={() => void importAnimatedPackage()}
          disabled={busy != null}
        >
          <FolderInput size={17} />
          {copy.importAnimated}
        </button>
      </section>
      {error && (
        <p className="desktop-pet-error" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <p className="pet-create-notice" role="status">
          {notice}
        </p>
      )}
    </div>
  );
}
