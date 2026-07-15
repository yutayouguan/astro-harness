/** 空会话欢迎卡片。 */
import type { ComponentType, SVGProps } from "react";
import { useI18n } from "../i18n/LocaleContext";
import type { MessageKey } from "../i18n/messages";
import { EmptyIllustration } from "../illustrations";
import {
  SolidBolt,
  SolidChat,
  SolidFolder,
  SolidStar,
} from "./GlassSolidIcons";

export type WelcomeCardId = "intro" | "skills" | "files" | "data";

/** 空会话欢迎卡片入参 */
type Props = {
  /** 点击卡片时把预设 prompt 填入输入框 */
  onPickCard: (prompt: string) => void;
};

type IconProps = SVGProps<SVGSVGElement>;

const CARDS: WelcomeCardId[] = ["intro", "skills", "files", "data"];

const CARD_META: Record<
  WelcomeCardId,
  {
    title: MessageKey;
    desc: MessageKey;
    prompt: MessageKey;
    tone: "blue" | "purple" | "cyan" | "orange";
    Icon: ComponentType<IconProps>;
  }
> = {
  intro: {
    title: "chat.card.intro.title",
    desc: "chat.card.intro.desc",
    prompt: "chat.card.intro.prompt",
    tone: "blue",
    Icon: SolidChat,
  },
  skills: {
    title: "chat.card.skills.title",
    desc: "chat.card.skills.desc",
    prompt: "chat.card.skills.prompt",
    tone: "purple",
    Icon: SolidStar,
  },
  files: {
    title: "chat.card.files.title",
    desc: "chat.card.files.desc",
    prompt: "chat.card.files.prompt",
    tone: "cyan",
    Icon: SolidFolder,
  },
  data: {
    title: "chat.card.data.title",
    desc: "chat.card.data.desc",
    prompt: "chat.card.data.prompt",
    tone: "orange",
    Icon: SolidBolt,
  },
};

export function ChatWelcome({ onPickCard }: Props) {
  const { t } = useI18n();
  const brandLabel = `${t("chat.welcomeGreeting")} Astro`;

  return (
    <div className="chat-empty chat-welcome" role="region" aria-label={brandLabel}>
      <div className="chat-welcome-hero" aria-hidden>
        <span className="chat-welcome-orb" />
        <span className="chat-welcome-orb chat-welcome-orb--soft" />
        <span className="chat-welcome-orb chat-welcome-orb--spark" />
      </div>

      <div className="chat-welcome-copy">
        <div className="chat-welcome-brand">
          <div className="chat-welcome-mark">
            <span className="chat-welcome-mark-glow" />
            <span className="chat-welcome-mark-ring" />
            <div className="chat-welcome-illust">
              <EmptyIllustration
                scene="chat"
                size="lg"
                className="chat-welcome-empty"
              />
            </div>
          </div>
          <p className="chat-welcome-wordmark">
            <span className="chat-welcome-wordmark-astro">Astro</span>
            <span className="chat-welcome-wordmark-agent">Agent</span>
          </p>
        </div>

        <h2 className="chat-welcome-title">
          <span className="chat-welcome-greeting">{t("chat.welcomeGreeting")}</span>{" "}
          <span className="chat-welcome-title-brand">Astro</span>
        </h2>
        <p className="chat-welcome-sub">{t("chat.welcomeSub")}</p>
      </div>

      <div className="chat-welcome-grid">
        {CARDS.map((id, index) => {
          const meta = CARD_META[id];
          const { Icon } = meta;
          return (
            <button
              key={id}
              type="button"
              className="chat-welcome-card"
              data-tone={meta.tone}
              style={{ animationDelay: `${0.08 + index * 0.06}s` }}
              onClick={() => onPickCard(t(meta.prompt))}
            >
              <span className="chat-welcome-card-icon" aria-hidden>
                <span className="chat-welcome-card-lens" />
                <span className="chat-welcome-card-glyph">
                  <Icon width={22} height={22} />
                </span>
              </span>
              <span className="chat-welcome-card-body">
                <span className="chat-welcome-card-title">{t(meta.title)}</span>
                <span className="chat-welcome-card-desc">{t(meta.desc)}</span>
              </span>
              <span className="chat-welcome-card-arrow" aria-hidden>
                →
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
