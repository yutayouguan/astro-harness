import type { WelcomeCardId } from "../../components/chat/ChatWelcome";

export type SoftWelcomeCategory = "all" | "work" | "create" | "explore";
export const SOFT_WELCOME_CATEGORIES = [
  "all",
  "work",
  "create",
  "explore",
] as const;
export const SOFT_WELCOME_GROUPS: Record<
  WelcomeCardId,
  Exclude<SoftWelcomeCategory, "all">
> = {
  intro: "explore",
  skills: "explore",
  files: "work",
  data: "work",
  image: "create",
  music: "create",
  video: "create",
  web: "explore",
  code: "work",
  writing: "work",
  search: "explore",
  translate: "work",
};

export function softWelcomeSections<T extends { id: WelcomeCardId }>(
  cards: readonly T[],
  category: SoftWelcomeCategory,
) {
  const visible = cards.filter(
    (card) => category === "all" || SOFT_WELCOME_GROUPS[card.id] === category,
  );
  const featured =
    category === "all"
      ? visible.filter((card) => card.id === "data" || card.id === "image")
      : [];
  const featuredIds = new Set(featured.map((card) => card.id));
  return {
    featured,
    cards: visible.filter((card) => !featuredIds.has(card.id)),
  };
}
