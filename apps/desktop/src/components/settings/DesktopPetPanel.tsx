import {
  Eye,
  EyeOff,
  FolderInput,
  ImagePlus,
  Loader2,
  PawPrint,
  Pin,
  Sparkles,
  Upload,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { useDesktopPetState } from "../../hooks/app/useDesktopPetState";
import { useReducedMotion } from "framer-motion";

import conceptImage from "../../assets/generated/desktop-pet-concept.png";
import DesktopPetCanvas from "../desktop-pet/DesktopPetCanvas";
import PetSceneLibrary from "./PetSceneLibrary";
import { useI18n } from "../../i18n/LocaleContext";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";

export type { DesktopPetState } from "../../lib/ui/desktopPetState";

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
    generatedHint: "可直接拖动桌宠改变它在屏幕上的位置",
    empty: "上传照片后即可生成",
    provider: "生成模型",
    importAnimated: "导入动画桌宠",
    importAnimatedHint: "选择 Codex v2 宠物包中的 pet.json",
    animatedBadge: "动画 v2",
    staticHint:
      "当前生成单张透明图片，不包含动画帧。眨眼、转头等动作需要导入完整 v2 动画宠物包。",
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
    generatedHint: "Drag the pet directly to move it around your screen",
    empty: "Upload a photo to start creating",
    provider: "Generated with",
    importAnimated: "Import animated pet",
    importAnimatedHint: "Choose pet.json from a Codex v2 pet package",
    animatedBadge: "Animated v2",
    staticHint:
      "Generation currently produces one transparent image, not animation frames. Import a complete v2 pet package for blinking, turning, and other motions.",
    staticBadge: "Static image",
  },
} as const;

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export default function DesktopPetPanel({ active }: { active: boolean }) {
  const { locale } = useI18n();
  const copy = COPY[locale];
  const reducedMotion = useReducedMotion();
  const {
    state,
    loading,
    pending,
    error: stateError,
    mutate,
    clearError,
  } = useDesktopPetState(active);
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
  const zh = locale === "zh";
  const [phase, setPhase] = useState<
    "upload" | "generate" | "wallpaper" | null
  >(null);
  const [localError, setLocalError] = useState("");
  const [scaleDraft, setScaleDraft] = useState<number | null>(null);
  const scaleCommit = useRef<number | null>(null);
  const busy = phase ?? (loading || pending > 0 ? "load" : null);
  const error = localError || stateError;
  useEffect(() => {
    setScaleDraft(null);
    setLocalError("");
  }, [state.revision]);

  async function choosePhoto() {
    if (busy) return;
    setPhase("upload");
    setLocalError("");
    clearError();
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
          ? "桌宠已保存到下方场景收藏，请预览后应用。"
          : "Pet saved below. Preview your scene before applying.",
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
    clearError();
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
      }
    } catch (cause) {
      setLocalError(errorMessage(cause));
    } finally {
      setPhase(null);
    }
  }

  async function updateToggle(
    command: "set_desktop_pet_enabled" | "set_desktop_pet_always_on_top",
    value: boolean,
  ) {
    setLocalError("");
    try {
      await mutate(
        command,
        command === "set_desktop_pet_enabled"
          ? { enabled: value }
          : { alwaysOnTop: value },
      );
    } catch {
      // Preserve the newest authoritative state, including concurrent Tool changes.
    }
  }

  async function updateScale(scale: number) {
    if (
      scaleCommit.current === scale ||
      (scale === state.scale && pending === 0)
    ) {
      setScaleDraft(null);
      return;
    }
    scaleCommit.current = scale;
    setLocalError("");
    try {
      await mutate("set_desktop_pet_scale", { scale });
    } catch {
      // The shared controller exposes the error without rolling back another update.
    } finally {
      if (scaleCommit.current === scale) {
        scaleCommit.current = null;
        setScaleDraft(null);
      }
    }
  }

  const sourceSrc = state.sourcePath ? resolveMediaSrc(state.sourcePath) : "";
  const petSrc = state.petPath ? resolveMediaSrc(state.petPath) : "";

  return (
    <section className="desktop-pet-settings" aria-busy={busy != null}>
      <div className="desktop-pet-hero">
        <div className="desktop-pet-hero-copy">
          <span className="desktop-pet-eyebrow">{copy.eyebrow}</span>
          <h2>{copy.title}</h2>
          <p>{copy.subtitle}</p>
        </div>
        <img src={conceptImage} alt="" aria-hidden />
      </div>

      <div className="desktop-pet-grid">
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
                zh
                  ? "例如：奶糖的森林小屋"
                  : "For example: Mochi's woodland home"
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

        <section className="prefs-card desktop-pet-card">
          <header className="desktop-pet-card-head">
            <span className="desktop-pet-icon" aria-hidden>
              <PawPrint size={19} />
            </span>
            <div>
              <h3>{copy.generated}</h3>
              <p>{copy.generatedHint}</p>
            </div>
          </header>
          <div className="desktop-pet-preview" data-empty={!petSrc}>
            {petSrc && state.spriteVersionNumber === 2 ? (
              <DesktopPetCanvas
                src={petSrc}
                state="idle"
                className="desktop-pet-preview-canvas"
                label={state.displayName || copy.generated}
                reducedMotion={Boolean(reducedMotion)}
              />
            ) : petSrc ? (
              <img src={petSrc} alt={copy.generated} />
            ) : (
              <div>
                <PawPrint size={34} />
                <span>{copy.empty}</span>
              </div>
            )}
          </div>
          {state.petPath ? (
            <div className="desktop-pet-animation-meta">
              <span>
                {state.spriteVersionNumber === 2
                  ? copy.animatedBadge
                  : copy.staticBadge}
              </span>
              <strong>{state.displayName || copy.generated}</strong>
            </div>
          ) : null}
          {state.provider && state.model ? (
            <p className="desktop-pet-model">
              {copy.provider}: {state.provider} · {state.model}
            </p>
          ) : null}
          <div className="desktop-pet-control-list">
            <label className="desktop-pet-toggle-row">
              <span className="desktop-pet-control-icon" aria-hidden>
                {state.enabled ? <Eye size={17} /> : <EyeOff size={17} />}
              </span>
              <span>
                <strong>{copy.show}</strong>
                <small>{copy.showHint}</small>
              </span>
              <input
                type="checkbox"
                checked={state.enabled}
                disabled={!state.petPath || busy != null}
                onChange={(event) =>
                  void updateToggle(
                    "set_desktop_pet_enabled",
                    event.currentTarget.checked,
                  )
                }
              />
            </label>
            <label className="desktop-pet-toggle-row">
              <span className="desktop-pet-control-icon" aria-hidden>
                <Pin size={17} />
              </span>
              <span>
                <strong>{copy.pin}</strong>
                <small>{copy.pinHint}</small>
              </span>
              <input
                type="checkbox"
                checked={state.alwaysOnTop}
                disabled={busy != null}
                onChange={(event) =>
                  void updateToggle(
                    "set_desktop_pet_always_on_top",
                    event.currentTarget.checked,
                  )
                }
              />
            </label>
            <label className="desktop-pet-size-row">
              <span>{copy.size}</span>
              <input
                type="range"
                min="0.65"
                max="1.35"
                step="0.05"
                value={scaleDraft ?? state.scale}
                disabled={busy != null}
                onChange={(event) => {
                  const scale = Number(event.currentTarget.value);
                  setScaleDraft(scale);
                }}
                onPointerUp={(event) =>
                  void updateScale(Number(event.currentTarget.value))
                }
                onKeyUp={(event) =>
                  void updateScale(Number(event.currentTarget.value))
                }
                onBlur={(event) =>
                  void updateScale(Number(event.currentTarget.value))
                }
              />
              <output>{Math.round((scaleDraft ?? state.scale) * 100)}%</output>
            </label>
          </div>
        </section>
      </div>
      <PetSceneLibrary
        state={state}
        mutate={mutate}
        busy={busy != null}
        zh={zh}
        name={sceneName}
        description={sceneDescription}
        includePet={includePet}
      />
    </section>
  );
}
