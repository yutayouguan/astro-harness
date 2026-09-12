import { useState } from "react";
import { ArrowUpRight } from "lucide-react";
import { AstroLogoMark } from "../icons/AstroLogoMark";
import { AIActionIcon } from "../icons/AIActionIcon";
import { useI18n } from "../../i18n/LocaleContext";
import { promptTemplateHints } from "../../lib/chat/promptTemplate";
import {
  SOFT_WELCOME_CATEGORIES,
  SOFT_WELCOME_GROUPS,
  softWelcomeSections,
  type SoftWelcomeCategory,
} from "../../lib/ui/softWelcome";
import type { WelcomeCard } from "./ChatWelcome";

type Props = {
  cards: readonly WelcomeCard[];
  onPickCard: (prompt: string, slotHints: string[]) => void;
};

function CardPreview({
  card,
  featured,
}: {
  card: WelcomeCard;
  featured: boolean;
}) {
  const Icon = card.meta.Icon;
  return (
    <span className="soft-welcome-preview" aria-hidden="true">
      {featured ? (
        <svg className="soft-welcome-art" viewBox="0 0 180 96" fill="none">
          <rect
            className="sw-art-back"
            x="28"
            y="15"
            width="124"
            height="70"
            rx="9"
            transform="rotate(-7 90 48)"
          />
          <rect
            className="sw-art-paper"
            x="28"
            y="13"
            width="124"
            height="70"
            rx="9"
          />
          {card.id === "data" ? (
            <>
              <path className="sw-art-grid" d="M42 30h96M42 48h96M42 66h96" />
              <rect
                className="sw-art-bar"
                x="47"
                y="49"
                width="16"
                height="22"
                rx="4"
              />
              <rect
                className="sw-art-bar"
                x="77"
                y="37"
                width="16"
                height="34"
                rx="4"
              />
              <rect
                className="sw-art-bar"
                x="107"
                y="24"
                width="16"
                height="47"
                rx="4"
              />
            </>
          ) : (
            <>
              <circle className="sw-art-sun" cx="122" cy="34" r="11" />
              <path className="sw-art-hill-back" d="M36 74 70 32 105 74Z" />
              <path className="sw-art-hill" d="M64 75 107 42 145 75Z" />
            </>
          )}
        </svg>
      ) : (
        <span className="soft-welcome-symbol">
          {card.id === "search" ? (
            <AIActionIcon size={30} variant="search" />
          ) : (
            <Icon width={30} height={30} />
          )}
        </span>
      )}
    </span>
  );
}

function ContentCard({
  card,
  featured = false,
  onPickCard,
}: {
  card: WelcomeCard;
  featured?: boolean;
  onPickCard: Props["onPickCard"];
}) {
  const { t } = useI18n();
  return (
    <button
      type="button"
      className={`soft-welcome-card${featured ? " is-featured" : ""}`}
      data-card={card.id}
      data-category={SOFT_WELCOME_GROUPS[card.id]}
      onClick={() => {
        const prompt = t(card.meta.prompt);
        onPickCard(prompt, promptTemplateHints(prompt));
      }}
    >
      <CardPreview card={card} featured={featured} />
      <span className="soft-welcome-card-copy">
        <strong>{t(card.meta.title)}</strong>
        <span>{t(card.meta.desc)}</span>
      </span>
      <ArrowUpRight className="soft-welcome-card-arrow" size={17} aria-hidden />
    </button>
  );
}

export function SoftWelcome({ cards, onPickCard }: Props) {
  const { t } = useI18n();
  const [category, setCategory] = useState<SoftWelcomeCategory>("all");
  const section = softWelcomeSections(cards, category);
  return (
    <section className="soft-welcome" aria-label={t("chat.softWelcome.title")}>
      <div className="soft-welcome-content">
        <header className="soft-welcome-header">
          <div>
            <span className="soft-welcome-brand">
              <AstroLogoMark width={24} height={24} aria-hidden />
              Astro
            </span>
            <h2>{t("chat.softWelcome.title")}</h2>
            <p>{t("chat.softWelcome.sub")}</p>
          </div>
        </header>
        <div
          className="soft-welcome-filters"
          role="group"
          aria-label={t("chat.softWelcome.categories")}
        >
          {SOFT_WELCOME_CATEGORIES.map((id) => (
            <button
              type="button"
              key={id}
              aria-pressed={category === id}
              onClick={() => setCategory(id)}
            >
              {t(`chat.softWelcome.${id}`)}
            </button>
          ))}
        </div>
        {section.featured.length ? (
          <div className="soft-welcome-featured">
            {section.featured.map((card) => (
              <ContentCard
                key={card.id}
                card={card}
                featured
                onPickCard={onPickCard}
              />
            ))}
          </div>
        ) : null}
        <div className="soft-welcome-grid">
          {section.cards.map((card) => (
            <ContentCard key={card.id} card={card} onPickCard={onPickCard} />
          ))}
        </div>
      </div>
    </section>
  );
}
