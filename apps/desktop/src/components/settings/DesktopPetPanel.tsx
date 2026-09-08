import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Eye,
  EyeOff,
  ImagePlus,
  Loader2,
  PawPrint,
  Pin,
  Sparkles,
  Upload,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import conceptImage from "../../assets/generated/desktop-pet-concept.png";
import { useI18n } from "../../i18n/LocaleContext";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";

export type DesktopPetState = {
  enabled: boolean;
  sourcePath: string | null;
  petPath: string | null;
  scale: number;
  alwaysOnTop: boolean;
  updatedAt: string;
  provider: string | null;
  model: string | null;
};

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
    create: "生成专属桌宠",
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
    create: "Create desktop pet",
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
  },
} as const;

const EMPTY_STATE: DesktopPetState = {
  enabled: false,
  sourcePath: null,
  petPath: null,
  scale: 1,
  alwaysOnTop: true,
  updatedAt: "",
  provider: null,
  model: null,
};

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export default function DesktopPetPanel({ active }: { active: boolean }) {
  const { locale } = useI18n();
  const copy = COPY[locale];
  const [state, setState] = useState<DesktopPetState>(EMPTY_STATE);
  const [description, setDescription] = useState("");
  const [busy, setBusy] = useState<"load" | "upload" | "generate" | null>(
    "load",
  );
  const [error, setError] = useState("");

  const refresh = useCallback(async () => {
    if (!("__TAURI_INTERNALS__" in window)) {
      setBusy(null);
      return;
    }
    try {
      setState(await invoke<DesktopPetState>("get_desktop_pet_state"));
      setError("");
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  }, []);

  useEffect(() => {
    if (active) void refresh();
  }, [active, refresh]);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<DesktopPetState>("desktop-pet-changed", (event) => {
      if (!disposed) setState(event.payload);
    })
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  async function choosePhoto() {
    setError("");
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
      if (typeof selected !== "string") return;
      setBusy("upload");
      setState(
        await invoke<DesktopPetState>("import_desktop_pet_photo", {
          sourcePath: selected,
        }),
      );
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  }

  async function generate() {
    if (!state.sourcePath || busy) return;
    setBusy("generate");
    setError("");
    try {
      setState(
        await invoke<DesktopPetState>("generate_desktop_pet", {
          description: description.trim() || null,
        }),
      );
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  }

  async function updateToggle(
    command: "set_desktop_pet_enabled" | "set_desktop_pet_always_on_top",
    value: boolean,
  ) {
    const previous = state;
    setState((current) =>
      command === "set_desktop_pet_enabled"
        ? { ...current, enabled: value }
        : { ...current, alwaysOnTop: value },
    );
    try {
      const args =
        command === "set_desktop_pet_enabled"
          ? { enabled: value }
          : { alwaysOnTop: value };
      setState(await invoke<DesktopPetState>(command, args));
      setError("");
    } catch (cause) {
      setState(previous);
      setError(errorMessage(cause));
    }
  }

  async function updateScale(scale: number) {
    setState((current) => ({ ...current, scale }));
    try {
      setState(
        await invoke<DesktopPetState>("set_desktop_pet_scale", { scale }),
      );
      setError("");
    } catch (cause) {
      setError(errorMessage(cause));
      void refresh();
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
          <button
            type="button"
            className="desktop-pet-generate"
            onClick={() => void generate()}
            disabled={!state.sourcePath || busy != null}
          >
            {busy === "generate" ? (
              <Loader2 className="desktop-pet-spinner" size={17} />
            ) : (
              <Sparkles size={17} />
            )}
            {busy === "generate" ? copy.creating : copy.create}
          </button>
          {error ? (
            <p className="desktop-pet-error" role="alert">
              {error}
            </p>
          ) : null}
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
            {petSrc ? (
              <img src={petSrc} alt={copy.generated} />
            ) : (
              <div>
                <PawPrint size={34} />
                <span>{copy.empty}</span>
              </div>
            )}
          </div>
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
                value={state.scale}
                disabled={busy != null}
                onChange={(event) => {
                  const scale = Number(event.currentTarget.value);
                  setState((current) => ({ ...current, scale }));
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
              <output>{Math.round(state.scale * 100)}%</output>
            </label>
          </div>
        </section>
      </div>
    </section>
  );
}
