/** 空会话欢迎卡片 — 双行跑马灯无限滚动。 */
import {
  useEffect,
  useId,
  useRef,
  useState,
  type ComponentType,
  type PointerEvent as ReactPointerEvent,
  type SVGProps,
} from "react";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import ParticleField from "./ParticleField";
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
import { Pause as PauseData, Play as PlayData } from "lucide";
import { MorphToggleIcon } from "../icons/MorphIcon";
import { WelcomeLogoEffect } from "./WelcomeLogoEffect";
import { promptTemplateHints } from "../../lib/chat/promptTemplate";
import { useInterfaceMaterial } from "../../hooks/app/useTheme";
import { SoftWelcome } from "./SoftWelcome";

const PAUSE_ICON = PauseData;
const PLAY_ICON = PlayData;

export type WelcomeCardId =
  | "intro"
  | "skills"
  | "files"
  | "data"
  | "image"
  | "music"
  | "video"
  | "web"
  | "code"
  | "writing"
  | "search"
  | "translate";

type Props = {
  onPickCard: (prompt: string, slotHints: string[]) => void;
  onActivate?: () => void;
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
  {
    id: "intro",
    meta: {
      title: "chat.card.intro.title",
      desc: "chat.card.intro.desc",
      prompt: "chat.card.intro.prompt",
      tone: "blue",
      Icon: SolidChat,
    },
  },
  {
    id: "skills",
    meta: {
      title: "chat.card.skills.title",
      desc: "chat.card.skills.desc",
      prompt: "chat.card.skills.prompt",
      tone: "purple",
      Icon: SolidStar,
    },
  },
  {
    id: "files",
    meta: {
      title: "chat.card.files.title",
      desc: "chat.card.files.desc",
      prompt: "chat.card.files.prompt",
      tone: "teal",
      Icon: SolidFolder,
    },
  },
  {
    id: "data",
    meta: {
      title: "chat.card.data.title",
      desc: "chat.card.data.desc",
      prompt: "chat.card.data.prompt",
      tone: "amber",
      Icon: SolidBolt,
    },
  },
  {
    id: "image",
    meta: {
      title: "chat.card.image.title" as MessageKey,
      desc: "chat.card.image.desc" as MessageKey,
      prompt: "chat.card.image.prompt" as MessageKey,
      tone: "rose",
      Icon: Image as ComponentType<IconProps>,
      lucide: true,
    },
  },
  {
    id: "music",
    meta: {
      title: "chat.card.music.title" as MessageKey,
      desc: "chat.card.music.desc" as MessageKey,
      prompt: "chat.card.music.prompt" as MessageKey,
      tone: "violet",
      Icon: Music as ComponentType<IconProps>,
      lucide: true,
    },
  },
  {
    id: "video",
    meta: {
      title: "chat.card.video.title" as MessageKey,
      desc: "chat.card.video.desc" as MessageKey,
      prompt: "chat.card.video.prompt" as MessageKey,
      tone: "indigo",
      Icon: Video as ComponentType<IconProps>,
      lucide: true,
    },
  },
  {
    id: "web",
    meta: {
      title: "chat.card.web.title" as MessageKey,
      desc: "chat.card.web.desc" as MessageKey,
      prompt: "chat.card.web.prompt" as MessageKey,
      tone: "emerald",
      Icon: Globe as ComponentType<IconProps>,
      lucide: true,
    },
  },
  {
    id: "code",
    meta: {
      title: "chat.card.code.title" as MessageKey,
      desc: "chat.card.code.desc" as MessageKey,
      prompt: "chat.card.code.prompt" as MessageKey,
      tone: "sky",
      Icon: Code as ComponentType<IconProps>,
      lucide: true,
    },
  },
  {
    id: "writing",
    meta: {
      title: "chat.card.writing.title" as MessageKey,
      desc: "chat.card.writing.desc" as MessageKey,
      prompt: "chat.card.writing.prompt" as MessageKey,
      tone: "pink",
      Icon: PenLine as ComponentType<IconProps>,
      lucide: true,
    },
  },
  {
    id: "search",
    meta: {
      title: "chat.card.search.title" as MessageKey,
      desc: "chat.card.search.desc" as MessageKey,
      prompt: "chat.card.search.prompt" as MessageKey,
      tone: "orange",
      Icon: Search as ComponentType<IconProps>,
      lucide: true,
    },
  },
  {
    id: "translate",
    meta: {
      title: "chat.card.translate.title" as MessageKey,
      desc: "chat.card.translate.desc" as MessageKey,
      prompt: "chat.card.translate.prompt" as MessageKey,
      tone: "cyan",
      Icon: Languages as ComponentType<IconProps>,
      lucide: true,
    },
  },
];

const ROW1 = ALL_CARDS.slice(0, 6);
const ROW2 = ALL_CARDS.slice(6, 12);
export type WelcomeCard = (typeof ALL_CARDS)[number];

type LogoDragState = {
  pointerId: number;
  startX: number;
  startY: number;
  originX: number;
  originY: number;
  x: number;
  y: number;
  lastX: number;
  lastY: number;
  lastAt: number;
  velocityX: number;
  velocityY: number;
  moved: boolean;
};

function currentTranslate(element: HTMLElement): { x: number; y: number } {
  const transform = window.getComputedStyle(element).transform;
  if (!transform || transform === "none") return { x: 0, y: 0 };
  try {
    const matrix = new DOMMatrixReadOnly(transform);
    return { x: matrix.m41, y: matrix.m42 };
  } catch {
    return { x: 0, y: 0 };
  }
}

function clampedMomentum(velocity: number): number {
  return Math.max(-18, Math.min(18, velocity * 0.025));
}

function MarqueeCard({
  card,
  onPick,
  duplicate = false,
}: {
  card: (typeof ALL_CARDS)[0];
  onPick: (prompt: string, slotHints: string[]) => void;
  duplicate?: boolean;
}) {
  const { t } = useI18n();
  const { meta } = card;
  const { Icon } = meta;
  return (
    <button
      type="button"
      className="chat-welcome-card"
      data-tone={meta.tone}
      onClick={() => {
        const prompt = t(meta.prompt);
        onPick(prompt, promptTemplateHints(prompt));
      }}
      tabIndex={duplicate ? -1 : undefined}
    >
      <span
        className={`chat-welcome-card-icon ${meta.lucide ? "is-lucide" : ""}`}
        aria-hidden
      >
        <span className="chat-welcome-card-lens" />
        <span className="chat-welcome-card-glyph">
          <Icon width={meta.lucide ? 20 : 22} height={meta.lucide ? 20 : 22} />
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
}

function MarqueeRow({
  cards,
  direction,
  onPick,
}: {
  cards: typeof ALL_CARDS;
  direction: "left" | "right";
  onPick: (prompt: string, slotHints: string[]) => void;
}) {
  return (
    <div className={`chat-welcome-marquee chat-welcome-marquee--${direction}`}>
      <div className="chat-welcome-marquee-track">
        <div className="chat-welcome-marquee-group">
          {cards.map((card) => (
            <MarqueeCard key={card.id} card={card} onPick={onPick} />
          ))}
        </div>
        <div
          className="chat-welcome-marquee-group chat-welcome-marquee-copy"
          aria-hidden="true"
        >
          {cards.map((card) => (
            <MarqueeCard key={card.id} card={card} onPick={onPick} duplicate />
          ))}
        </div>
      </div>
    </div>
  );
}

export function ChatWelcome(props: Props) {
  const material = useInterfaceMaterial();
  return material === "soft" ? (
    <SoftWelcome cards={ALL_CARDS} {...props} />
  ) : (
    <GlassWelcome {...props} />
  );
}

function GlassWelcome({ onPickCard, onActivate }: Props) {
  const { t } = useI18n();
  const subtitleId = useId();
  const [marqueePaused, setMarqueePaused] = useState(false);
  const [pulseId, setPulseId] = useState(0);
  const [burstId, setBurstId] = useState(0);
  const dragLayerRef = useRef<HTMLSpanElement>(null);
  const dragRef = useRef<LogoDragState | null>(null);
  const dragReturnRef = useRef<Animation | null>(null);
  const suppressLogoClickRef = useRef(false);
  const brandLabel = `${t("chat.welcomeGreeting")} Astro`;
  const marqueeControlLabel = t(marqueePaused ? "media.play" : "media.pause");

  useEffect(() => () => dragReturnRef.current?.cancel(), []);

  const handleLogoPointerMove = (
    event: ReactPointerEvent<HTMLButtonElement>,
  ) => {
    const drag = dragRef.current;
    if (drag?.pointerId === event.pointerId && dragLayerRef.current) {
      const now = performance.now();
      const elapsed = Math.max(1, now - drag.lastAt);
      drag.x = drag.originX + event.clientX - drag.startX;
      drag.y = drag.originY + event.clientY - drag.startY;
      drag.velocityX = ((event.clientX - drag.lastX) / elapsed) * 1000;
      drag.velocityY = ((event.clientY - drag.lastY) / elapsed) * 1000;
      drag.lastX = event.clientX;
      drag.lastY = event.clientY;
      drag.lastAt = now;
      drag.moved ||=
        Math.hypot(event.clientX - drag.startX, event.clientY - drag.startY) >
        5;
      dragLayerRef.current.style.transform = `translate3d(${drag.x}px, ${drag.y}px, 0)`;
      return;
    }

    if (event.pointerType === "touch") return;
    const bounds = event.currentTarget.getBoundingClientRect();
    const x = (event.clientX - bounds.left) / bounds.width - 0.5;
    const y = (event.clientY - bounds.top) / bounds.height - 0.5;
    event.currentTarget.style.setProperty(
      "--welcome-tilt-x",
      `${(-y * 10).toFixed(2)}deg`,
    );
    event.currentTarget.style.setProperty(
      "--welcome-tilt-y",
      `${(x * 12).toFixed(2)}deg`,
    );
    event.currentTarget.style.setProperty(
      "--welcome-glare-x",
      `${Math.round((x + 0.5) * 100)}%`,
    );
    event.currentTarget.style.setProperty(
      "--welcome-glare-y",
      `${Math.round((y + 0.5) * 100)}%`,
    );
  };

  const resetLogoTilt = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (dragRef.current) return;
    event.currentTarget.style.removeProperty("--welcome-tilt-x");
    event.currentTarget.style.removeProperty("--welcome-tilt-y");
    event.currentTarget.style.removeProperty("--welcome-glare-x");
    event.currentTarget.style.removeProperty("--welcome-glare-y");
  };

  const startLogoDrag = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (event.button !== 0 || !dragLayerRef.current) return;
    const layer = dragLayerRef.current;
    const origin = currentTranslate(layer);
    dragReturnRef.current?.cancel();
    dragReturnRef.current = null;
    layer.style.transform = `translate3d(${origin.x}px, ${origin.y}px, 0)`;
    event.currentTarget.setPointerCapture(event.pointerId);
    event.currentTarget.classList.add("is-dragging");
    dragRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      originX: origin.x,
      originY: origin.y,
      x: origin.x,
      y: origin.y,
      lastX: event.clientX,
      lastY: event.clientY,
      lastAt: performance.now(),
      velocityX: 0,
      velocityY: 0,
      moved: false,
    };
  };

  const finishLogoDrag = (event: ReactPointerEvent<HTMLButtonElement>) => {
    const drag = dragRef.current;
    const layer = dragLayerRef.current;
    if (!drag || drag.pointerId !== event.pointerId || !layer) return;
    dragRef.current = null;
    event.currentTarget.classList.remove("is-dragging");
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }

    suppressLogoClickRef.current = drag.moved;
    window.setTimeout(() => {
      suppressLogoClickRef.current = false;
    }, 0);

    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      layer.style.removeProperty("transform");
      return;
    }

    const peakX = drag.x + clampedMomentum(drag.velocityX);
    const peakY = drag.y + clampedMomentum(drag.velocityY);
    const distance = Math.hypot(drag.x, drag.y);
    dragReturnRef.current = layer.animate(
      [
        { transform: `translate3d(${drag.x}px, ${drag.y}px, 0)`, offset: 0 },
        { transform: `translate3d(${peakX}px, ${peakY}px, 0)`, offset: 0.16 },
        {
          transform: `translate3d(${-drag.x * 0.07}px, ${-drag.y * 0.07}px, 0)`,
          offset: 0.72,
        },
        { transform: "translate3d(0, 0, 0)", offset: 1 },
      ],
      {
        duration: Math.min(600, 420 + distance * 1.15),
        easing: "cubic-bezier(0.22, 0.8, 0.24, 1)",
        fill: "both",
      },
    );
    dragReturnRef.current.addEventListener(
      "finish",
      () => {
        layer.style.removeProperty("transform");
        dragReturnRef.current = null;
      },
      { once: true },
    );
  };

  const activateLogo = () => {
    if (suppressLogoClickRef.current) {
      suppressLogoClickRef.current = false;
      return;
    }
    setPulseId((value) => value + 1);
    setBurstId((value) => value + 1);
    onActivate?.();
  };

  return (
    <div
      className="chat-empty chat-welcome"
      role="region"
      aria-label={brandLabel}
    >
      <div className="chat-welcome-hero" aria-hidden>
        <span className="chat-welcome-orb" />
        <span className="chat-welcome-orb chat-welcome-orb--soft" />
        <span className="chat-welcome-orb chat-welcome-orb--spark" />
        <ParticleField />
      </div>

      <div className="chat-welcome-copy">
        <div className="chat-welcome-brand">
          <button
            type="button"
            className="chat-welcome-mark"
            aria-label={brandLabel}
            aria-describedby={subtitleId}
            title={t("chat.welcomePlaceholder")}
            onPointerDown={startLogoDrag}
            onPointerMove={handleLogoPointerMove}
            onPointerUp={finishLogoDrag}
            onPointerCancel={finishLogoDrag}
            onLostPointerCapture={finishLogoDrag}
            onPointerLeave={resetLogoTilt}
            onClick={activateLogo}
            onDragStart={(event) => event.preventDefault()}
          >
            <span ref={dragLayerRef} className="chat-welcome-drag-layer">
              <span className="chat-welcome-mark-glow" />
              <span className="chat-welcome-illust">
                <WelcomeLogoEffect />
              </span>
              {pulseId > 0 ? (
                <span
                  key={pulseId}
                  className="chat-welcome-mark-pulse"
                  aria-hidden
                />
              ) : null}
              {burstId > 0 ? (
                <span key={burstId} className="chat-welcome-burst" aria-hidden>
                  {Array.from({ length: 12 }, (_, index) => (
                    <i key={index} />
                  ))}
                </span>
              ) : null}
            </span>
          </button>
          <p className="chat-welcome-wordmark">
            <span className="chat-welcome-wordmark-astro">Astro</span>
            <span className="chat-welcome-wordmark-agent">Agent</span>
          </p>
        </div>

        <h2 className="chat-welcome-title">
          <span className="chat-welcome-greeting">
            {t("chat.welcomeGreeting")}
          </span>{" "}
          <span className="chat-welcome-title-brand">Astro</span>
        </h2>
        <p id={subtitleId} className="chat-welcome-sub">
          {t("chat.welcomeSub")}
        </p>
      </div>

      <div className="chat-welcome-marquee-region">
        <button
          type="button"
          className="chat-welcome-marquee-control"
          aria-pressed={marqueePaused}
          aria-label={marqueeControlLabel}
          title={marqueeControlLabel}
          onClick={() => setMarqueePaused((paused) => !paused)}
        >
          <MorphToggleIcon
            active={marqueePaused}
            activeIcon={PLAY_ICON}
            inactiveIcon={PAUSE_ICON}
            size={13}
            aria-hidden
          />
        </button>

        <div
          className={`chat-welcome-marquee-wrap ${marqueePaused ? "is-paused" : ""}`}
        >
          <MarqueeRow cards={ROW1} direction="right" onPick={onPickCard} />
          <MarqueeRow cards={ROW2} direction="left" onPick={onPickCard} />
        </div>
      </div>
    </div>
  );
}
