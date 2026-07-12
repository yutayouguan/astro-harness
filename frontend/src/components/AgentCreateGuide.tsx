/** 创建 Agent 引导与图标上传。 */
import { useEffect, useRef, useState, type MutableRefObject } from "react";
import { X } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n/LocaleContext";
import { agentNameInitial } from "../lib/agentIcons";
import {
  lucideIconToSvgBase64Async,
  type LucideAgentIcon,
  type LucidePaint,
  type LucideRenderStyle,
} from "../lib/lucideAgentIcons";
import { IconSparkles, IconSkills, IconWorkspace } from "./NavIcons";
import LucideIconPicker from "./LucideIconPicker";

/** 创建 Agent 引导卡片入参 */
type Props = {
  /** 跳过引导 */
  onSkip: () => void;
  /** 预览用的名称首字（来自输入框模板里的名称槽，可为空） */
  previewName?: string;
};

/** 图标槽位种类 */
type IconKind = "emoji" | "avatar";

/** 单个图标上传/选择槽状态 */
type SlotState = {
  previewUrl: string | null;
  fileName: string | null;
  lucideId?: string | null;
};

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function fileToBase64(file: File): Promise<string> {
  const buf = await file.arrayBuffer();
  const bytes = new Uint8Array(buf);
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

export function AgentCreateGuide({ onSkip, previewName = "" }: Props) {
  const { t } = useI18n();
  const [emoji, setEmoji] = useState<SlotState>({ previewUrl: null, fileName: null });
  const [avatar, setAvatar] = useState<SlotState>({ previewUrl: null, fileName: null });
  const [error, setError] = useState<string | null>(null);
  const [lucideOpen, setLucideOpen] = useState(false);
  const [lucideBusy, setLucideBusy] = useState(false);
  const emojiInputRef = useRef<HTMLInputElement | null>(null);
  const avatarInputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    return () => {
      if (emoji.previewUrl) URL.revokeObjectURL(emoji.previewUrl);
      if (avatar.previewUrl) URL.revokeObjectURL(avatar.previewUrl);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only revoke on unmount
  }, []);

  const pick = async (kind: IconKind, file: File | null) => {
    if (!file) return;
    if (!file.type.startsWith("image/")) {
      setError(t("chat.agentIconInvalid"));
      return;
    }
    setError(null);
    const previewUrl = URL.createObjectURL(file);
    const setter = kind === "emoji" ? setEmoji : setAvatar;
    setter((prev) => {
      if (prev.previewUrl) URL.revokeObjectURL(prev.previewUrl);
      return { previewUrl, fileName: file.name, lucideId: null };
    });
    if (!isTauri()) return;
    try {
      const dataBase64 = await fileToBase64(file);
      await invoke("set_pending_agent_icon", {
        kind,
        dataBase64,
        fileName: file.name,
      });
    } catch (e) {
      setError(String(e));
    }
  };

  const clear = async (kind: IconKind) => {
    const setter = kind === "emoji" ? setEmoji : setAvatar;
    setter((prev) => {
      if (prev.previewUrl) URL.revokeObjectURL(prev.previewUrl);
      return { previewUrl: null, fileName: null, lucideId: null };
    });
    if (!isTauri()) return;
    try {
      await invoke("clear_pending_agent_icon", { kind });
    } catch {
      // ignore
    }
  };

  const pickLucide = async (
    icon: LucideAgentIcon,
    paint: LucidePaint,
    style: LucideRenderStyle,
  ) => {
    setLucideBusy(true);
    setError(null);
    try {
      const dataBase64 = await lucideIconToSvgBase64Async(icon.Icon, { paint, style });
      const binary = atob(dataBase64);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
      const blob = new Blob([bytes], { type: "image/svg+xml" });
      const previewUrl = URL.createObjectURL(blob);
      const fileName = `emoji-${icon.id}.svg`;

      setEmoji((prev) => {
        if (prev.previewUrl) URL.revokeObjectURL(prev.previewUrl);
        return { previewUrl, fileName, lucideId: icon.id };
      });

      if (isTauri()) {
        await invoke("set_pending_agent_icon", {
          kind: "emoji",
          dataBase64,
          fileName,
        });
      }
      setLucideOpen(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setLucideBusy(false);
    }
  };

  const initial = agentNameInitial(previewName || t("chat.agentIconFallbackName"));
  const heroPreview = avatar.previewUrl || emoji.previewUrl;

  const steps = [
    { key: "fill", icon: <IconSkills width={13} height={13} />, label: t("chat.agentGuideStep1") },
    { key: "icon", icon: <IconSparkles width={13} height={13} />, label: t("chat.agentGuideStep2") },
    { key: "send", icon: <IconWorkspace width={13} height={13} />, label: t("chat.agentGuideStep3") },
  ] as const;

  const renderSlot = (
    kind: IconKind,
    state: SlotState,
    inputRef: MutableRefObject<HTMLInputElement | null>,
    label: string,
    hint: string,
    tone: "emoji" | "avatar",
    delay: string,
  ) => (
    <div
      className={`chat-agent-icon-slot tone-${tone}`}
      data-tone={tone === "emoji" ? "blue" : "purple"}
      style={{ animationDelay: delay }}
    >
      <button
        type="button"
        className={`chat-agent-icon-preview ${state.previewUrl ? "has-image" : ""}`}
        onClick={() => {
          if (kind === "emoji") setLucideOpen(true);
          else inputRef.current?.click();
        }}
        aria-label={label}
      >
        <span className="chat-agent-icon-preview-glow" aria-hidden />
        {state.previewUrl ? (
          <img src={state.previewUrl} alt="" draggable={false} />
        ) : (
          <>
            <span className="chat-agent-icon-fallback">{initial}</span>
            <span className="chat-agent-icon-plus" aria-hidden>
              +
            </span>
          </>
        )}
      </button>
      <div className="chat-agent-icon-slot-meta">
        <span className="chat-agent-icon-slot-label">{label}</span>
        <span className="chat-agent-icon-slot-hint">{hint}</span>
        <div className="chat-agent-icon-actions">
          {kind === "emoji" ? (
            <>
              <button
                type="button"
                className="chat-agent-icon-btn"
                onClick={() => setLucideOpen(true)}
                disabled={lucideBusy}
              >
                {state.lucideId
                  ? t("chat.agentIconLucideChange")
                  : t("chat.agentIconLucidePick")}
              </button>
              <button
                type="button"
                className="chat-agent-icon-btn subtle"
                onClick={() => inputRef.current?.click()}
                disabled={lucideBusy}
              >
                {t("chat.agentIconUpload")}
              </button>
            </>
          ) : (
            <button
              type="button"
              className="chat-agent-icon-btn"
              onClick={() => inputRef.current?.click()}
            >
              {state.previewUrl ? t("chat.agentIconReplace") : t("chat.agentIconUpload")}
            </button>
          )}
          {state.previewUrl ? (
            <button
              type="button"
              className="chat-agent-icon-btn subtle"
              onClick={() => void clear(kind)}
              disabled={lucideBusy}
            >
              {t("chat.agentIconClear")}
            </button>
          ) : null}
        </div>
        {state.fileName ? <span className="chat-agent-icon-filename">{state.fileName}</span> : null}
      </div>
      <input
        ref={inputRef}
        type="file"
        accept="image/png,image/jpeg,image/webp,image/gif,image/svg+xml,.png,.jpg,.jpeg,.webp,.gif,.svg"
        hidden
        onChange={(e) => {
          const file = e.target.files?.[0] ?? null;
          e.target.value = "";
          void pick(kind, file);
        }}
      />
    </div>
  );

  return (
    <div className="chat-empty chat-agent-guide" role="region" aria-label={t("chat.agentGuideTitle")}>
      <div className="chat-agent-guide-ambient" aria-hidden>
        <span className="chat-agent-guide-orb" />
        <span className="chat-agent-guide-orb chat-agent-guide-orb--soft" />
      </div>
      <div className="chat-agent-guide-card">
        <button
          type="button"
          className="chat-agent-guide-close"
          onClick={onSkip}
          title={t("chat.agentGuideCancel")}
          aria-label={t("chat.agentGuideCancel")}
        >
          <X size={16} strokeWidth={2} aria-hidden />
        </button>
        <div className="chat-agent-guide-hero">
          <div className="chat-agent-guide-monogram" aria-hidden>
            <span className="chat-agent-guide-monogram-ring" />
            {heroPreview ? (
              <img src={heroPreview} alt="" draggable={false} />
            ) : (
              <span className="chat-agent-guide-monogram-text">{initial}</span>
            )}
          </div>
          <div className="chat-agent-guide-intro">
            <span className="chat-agent-guide-eyebrow">
              <IconSparkles width={14} height={14} />
              {t("chat.agentGuideEyebrow")}
            </span>
            <h2 className="chat-agent-guide-title">{t("chat.agentGuideTitle")}</h2>
            <p className="chat-agent-guide-body">{t("chat.agentGuideBody")}</p>
          </div>
        </div>

        <ol className="chat-agent-guide-steps" aria-label={t("chat.agentGuideStepsLabel")}>
          {steps.map((step, i) => (
            <li
              key={step.key}
              className="chat-agent-guide-step"
              data-step={i + 1}
              style={{ animationDelay: `${0.06 + i * 0.05}s` }}
            >
              <span className="chat-agent-guide-step-index" aria-hidden>
                {step.icon}
              </span>
              <span className="chat-agent-guide-step-label">
                <span className="chat-agent-guide-step-num">{i + 1}</span>
                {step.label}
              </span>
            </li>
          ))}
        </ol>

        <div className="chat-agent-icons">
          <div className="chat-agent-icons-head">
            <p className="chat-agent-icons-title">{t("chat.agentIconsTitle")}</p>
            <p className="chat-agent-icons-sub">{t("chat.agentIconsSub")}</p>
          </div>
          <div className="chat-agent-icons-grid">
            {renderSlot(
              "emoji",
              emoji,
              emojiInputRef,
              t("chat.agentIconEmoji"),
              t("chat.agentIconEmojiHint"),
              "emoji",
              "0.14s",
            )}
            {renderSlot(
              "avatar",
              avatar,
              avatarInputRef,
              t("chat.agentIconAvatar"),
              t("chat.agentIconAvatarHint"),
              "avatar",
              "0.2s",
            )}
          </div>
          {error ? <p className="chat-agent-icons-error">{error}</p> : null}
        </div>

        <button type="button" className="chat-agent-guide-skip" onClick={onSkip}>
          {t("chat.agentGuideSkip")}
        </button>
      </div>

      <LucideIconPicker
        open={lucideOpen}
        selectedId={emoji.lucideId}
        onClose={() => setLucideOpen(false)}
        onSelect={(icon, paint, style) => void pickLucide(icon, paint, style)}
      />
    </div>
  );
}
