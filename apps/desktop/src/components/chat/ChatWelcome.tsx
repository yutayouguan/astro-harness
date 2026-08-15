/** 空会话欢迎卡片 — 多组轮播。 */
import { useCallback, useEffect, useRef, useState, type ComponentType, type SVGProps } from "react";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import { AstroLogoMark } from "../icons/AstroLogoMark";
import {
  SolidBolt,
  SolidChat,
  SolidFolder,
  SolidStar,
} from "../icons/GlassSolidIcons";
import {
  Image,
  Music,
  Video,
  Globe,
  Code,
  PenLine,
  Search,
  Languages,
} from "lucide-react";

export type WelcomeCardId =
  | "intro" | "skills" | "files" | "data"
  | "image" | "music" | "video" | "web"
  | "code" | "writing" | "search" | "translate";

type Props = {
  onPickCard: (prompt: string) => void;
};

type IconProps = SVGProps<SVGSVGElement>;

type CardMeta = {
  title: MessageKey;
  desc: MessageKey;
  prompt: MessageKey;
  tone: string;
  Icon: ComponentType<IconProps>;
  lucide?: boolean;
};

const ALL_CARDS: { id: WelcomeCardId; meta: CardMeta }[] = [
  // Group 1: Getting started
  { id: "intro", meta: { title: "chat.card.intro.title", desc: "chat.card.intro.desc", prompt: "chat.card.intro.prompt", tone: "blue", Icon: SolidChat } },
  { id: "skills", meta: { title: "chat.card.skills.title", desc: "chat.card.skills.desc", prompt: "chat.card.skills.prompt", tone: "purple", Icon: SolidStar } },
  { id: "files", meta: { title: "chat.card.files.title", desc: "chat.card.files.desc", prompt: "chat.card.files.prompt", tone: "teal", Icon: SolidFolder } },
  { id: "data", meta: { title: "chat.card.data.title", desc: "chat.card.data.desc", prompt: "chat.card.data.prompt", tone: "amber", Icon: SolidBolt } },
  // Group 2: Creative
  { id: "image", meta: { title: "chat.card.image.title" as MessageKey, desc: "chat.card.image.desc" as MessageKey, prompt: "chat.card.image.prompt" as MessageKey, tone: "rose", Icon: Image as ComponentType<IconProps>, lucide: true } },
  { id: "music", meta: { title: "chat.card.music.title" as MessageKey, desc: "chat.card.music.desc" as MessageKey, prompt: "chat.card.music.prompt" as MessageKey, tone: "violet", Icon: Music as ComponentType<IconProps>, lucide: true } },
  { id: "video", meta: { title: "chat.card.video.title" as MessageKey, desc: "chat.card.video.desc" as MessageKey, prompt: "chat.card.video.prompt" as MessageKey, tone: "indigo", Icon: Video as ComponentType<IconProps>, lucide: true } },
  { id: "web", meta: { title: "chat.card.web.title" as MessageKey, desc: "chat.card.web.desc" as MessageKey, prompt: "chat.card.web.prompt" as MessageKey, tone: "emerald", Icon: Globe as ComponentType<IconProps>, lucide: true } },
  // Group 3: Productivity
  { id: "code", meta: { title: "chat.card.code.title" as MessageKey, desc: "chat.card.code.desc" as MessageKey, prompt: "chat.card.code.prompt" as MessageKey, tone: "sky", Icon: Code as ComponentType<IconProps>, lucide: true } },
  { id: "writing", meta: { title: "chat.card.writing.title" as MessageKey, desc: "chat.card.writing.desc" as MessageKey, prompt: "chat.card.writing.prompt" as MessageKey, tone: "pink", Icon: PenLine as ComponentType<IconProps>, lucide: true } },
  { id: "search", meta: { title: "chat.card.search.title" as MessageKey, desc: "chat.card.search.desc" as MessageKey, prompt: "chat.card.search.prompt" as MessageKey, tone: "orange", Icon: Search as ComponentType<IconProps>, lucide: true } },
  { id: "translate", meta: { title: "chat.card.translate.title" as MessageKey, desc: "chat.card.translate.desc" as MessageKey, prompt: "chat.card.translate.prompt" as MessageKey, tone: "cyan", Icon: Languages as ComponentType<IconProps>, lucide: true } },
];

const GROUPS = [
  ALL_CARDS.slice(0, 4),
  ALL_CARDS.slice(4, 8),
  ALL_CARDS.slice(8, 12),
];

const INTERVAL_MS = 5000;

export function ChatWelcome({ onPickCard }: Props) {
  const { t } = useI18n();
  const brandLabel = `${t("chat.welcomeGreeting")} Astro`;
  const [activeGroup, setActiveGroup] = useState(0);
  const [direction, setDirection] = useState<"next" | "prev">("next");
  const [animating, setAnimating] = useState(false);
  const timerRef = useRef<number | null>(null);

  const goTo = useCallback((idx: number, dir: "next" | "prev") => {
    if (animating) return;
    setDirection(dir);
    setAnimating(true);
    setTimeout(() => {
      setActiveGroup(idx);
      setTimeout(() => setAnimating(false), 50);
    }, 280);
  }, [animating]);

  const next = useCallback(() => {
    goTo((activeGroup + 1) % GROUPS.length, "next");
  }, [activeGroup, goTo]);

  useEffect(() => {
    timerRef.current = window.setInterval(next, INTERVAL_MS);
    return () => { if (timerRef.current) clearInterval(timerRef.current); };
  }, [next]);

  const onDotClick = (idx: number) => {
    if (idx === activeGroup) return;
    goTo(idx, idx > activeGroup ? "next" : "prev");
    if (timerRef.current) clearInterval(timerRef.current);
    timerRef.current = window.setInterval(next, INTERVAL_MS);
  };

  const group = GROUPS[activeGroup];

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
            <div className="chat-welcome-illust">
              <AstroLogoMark className="chat-welcome-logo" width={72} height={72} />
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

      <div className={`chat-welcome-carousel ${animating ? `is-leaving-${direction}` : "is-entering"}`}>
        <div className="chat-welcome-grid" key={activeGroup}>
          {group.map((card, index) => {
            const { meta } = card;
            const { Icon } = meta;
            return (
              <button
                key={card.id}
                type="button"
                className="chat-welcome-card"
                data-tone={meta.tone}
                style={{ animationDelay: `${index * 0.07}s` }}
                onClick={() => onPickCard(t(meta.prompt))}
              >
                <span className={`chat-welcome-card-icon ${meta.lucide ? "is-lucide" : ""}`} aria-hidden>
                  <span className="chat-welcome-card-lens" />
                  <span className="chat-welcome-card-glyph">
                    <Icon width={meta.lucide ? 20 : 22} height={meta.lucide ? 20 : 22} />
                  </span>
                </span>
                <span className="chat-welcome-card-body">
                  <span className="chat-welcome-card-title">{t(meta.title)}</span>
                  <span className="chat-welcome-card-desc">{t(meta.desc)}</span>
                </span>
                <span className="chat-welcome-card-arrow" aria-hidden>→</span>
              </button>
            );
          })}
        </div>
      </div>

      <div className="chat-welcome-dots" aria-label="carousel pagination">
        {GROUPS.map((_, idx) => (
          <button
            key={idx}
            type="button"
            className={`chat-welcome-dot ${idx === activeGroup ? "is-active" : ""}`}
            onClick={() => onDotClick(idx)}
            aria-label={`Page ${idx + 1}`}
            aria-current={idx === activeGroup ? "step" : undefined}
          />
        ))}
      </div>
    </div>
  );
}
