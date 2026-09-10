import {
  FolderInput,
  ImagePlus,
  Loader2,
  PawPrint,
  Sparkles,
  Upload,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useRef, useState } from "react";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import type { DesktopPetState } from "../../lib/ui/desktopPetState";
const COPY = {
  zh: {
    eyebrow: "ASTRO DESKTOP COMPANION",
    title: "把熟悉的它，带到桌面上",
    subtitle:
      "上传一张清晰的宠物照片，使用当前图片模型生成保留外貌特征的专属桌宠。点击生成时，照片会发送给你选择的图片 Provider；本地副本与结果保存在本机。",
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
      "当前照片生成不包含动画帧。可使用内置奶糖，或导入含独立动作片段的动画宠物包。",
    staticBadge: "静态图片",
  },
  en: {
    eyebrow: "ASTRO DESKTOP COMPANION",
    title: "Bring a familiar friend to your desktop",
    subtitle:
      "Upload a clear pet photo and use your active image model to create a personal desktop companion. When you generate, the photo is sent to your selected image provider; the local copy and result stay on this device.",
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
      "Photo generation produces a static portrait. Use built-in Naitang or import an animation package with independent motion clips.",
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
      <header>
        <h3>{zh ? "让熟悉的它，成为你的桌面伙伴" : "Create your companion"}</h3>
        <p>{copy.subtitle}</p>
      </header>
      <section className="prefs-card desktop-pet-card">
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
          onClick={() => void choosePhoto()}
          disabled={busy != null}
        >
          {sourceSrc ? (
            <img src={sourceSrc} alt={copy.source} />
          ) : (
            <PawPrint size={42} />
          )}
          <span>
            <Upload size={15} />
            {sourceSrc ? copy.replace : copy.upload}
          </span>
        </button>
        <label className="desktop-pet-field">
          <span>{copy.style}</span>
          <textarea
            value={description}
            onChange={(event) => setDescription(event.currentTarget.value)}
            placeholder={copy.stylePlaceholder}
            maxLength={2000}
            disabled={busy != null}
          />
        </label>
        <label className="desktop-pet-field">
          <span>{zh ? "宠物名字" : "Pet name"}</span>
          <input
            value={petName}
            maxLength={80}
            disabled={busy != null}
            placeholder={zh ? "例如：奶糖" : "For example: Mochi"}
            onChange={(e) => setPetName(e.currentTarget.value)}
          />
        </label>
        <label className="desktop-pet-field">
          <span>{zh ? "场景名称" : "Scene name"}</span>
          <input
            value={sceneName}
            maxLength={80}
            disabled={busy != null}
            placeholder={
              zh ? "例如：奶糖的森林小屋" : "For example: Mochi's woodland home"
            }
            onChange={(e) => setSceneName(e.currentTarget.value)}
          />
        </label>
        <label className="desktop-pet-toggle-row">
          <span>
            <strong>
              {zh ? "同时生成配套壁纸" : "Generate a matching wallpaper"}
            </strong>
            <small>
              {zh
                ? "会额外调用一次图片模型；只用于 Astro 应用背景。"
                : "One additional image request. Astro background only."}
            </small>
          </span>
          <input
            type="checkbox"
            checked={withWallpaper}
            disabled={busy != null}
            onChange={(e) => setWithWallpaper(e.currentTarget.checked)}
          />
        </label>
        <details
          className="pet-scene-options"
          open={withWallpaper || undefined}
        >
          <summary>
            {zh
              ? "配套壁纸选项（也用于下方重试）"
              : "Wallpaper options (also used for retries)"}
          </summary>
          <label className="desktop-pet-field">
            <span>{zh ? "场景描述" : "Scene description"}</span>
            <textarea
              value={sceneDescription}
              maxLength={2000}
              disabled={busy != null}
              placeholder={
                zh
                  ? "森林小屋、海边日落、星空花园…"
                  : "Woodland home, sunset beach, starry garden…"
              }
              onChange={(e) => setSceneDescription(e.currentTarget.value)}
            />
          </label>
          <label className="desktop-pet-toggle-row">
            <span>
              <strong>
                {zh ? "壁纸中包含宠物肖像" : "Include a pet portrait"}
              </strong>
              <small>
                {zh
                  ? "默认只生成环境，避免与悬浮桌宠重复。"
                  : "Environment-only by default, to avoid duplicating the floating pet."}
              </small>
            </span>
            <input
              type="checkbox"
              checked={includePet}
              disabled={busy != null}
              onChange={(e) => setIncludePet(e.currentTarget.checked)}
            />
          </label>
        </details>
        <div className="desktop-pet-action-row">
          <button
            type="button"
            className="desktop-pet-generate"
            onClick={() => void generate()}
            disabled={!state.sourcePath || busy != null}
          >
            {phase === "generate" || phase === "wallpaper" ? (
              <Loader2 className="desktop-pet-spinner" size={17} />
            ) : (
              <Sparkles size={17} />
            )}
            {phase === "wallpaper"
              ? zh
                ? "正在生成配套壁纸…"
                : "Creating matching wallpaper…"
              : phase === "generate"
                ? copy.creating
                : copy.create}
          </button>
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
        </div>
        <p className="desktop-pet-model">{copy.staticHint}</p>
        {error ? (
          <p className="desktop-pet-error" role="alert">
            {error}
          </p>
        ) : null}
        {notice && <p role="status">{notice}</p>}
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
      </section>
    </div>
  );
}
