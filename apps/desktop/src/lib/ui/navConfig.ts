import type { ComponentType, SVGProps } from "react";
import {
  IconChat,
  IconCron,
  IconLoop,
  IconPlugin,
  IconSettings,
} from "../../components/icons";
import type { MessageKey } from "../../i18n/messages";

export type NavId = "chat" | "cron" | "loop" | "skills" | "settings";

export type SettingsTabId =
  | "preferences"
  | "preferences:appearance"
  | "preferences:conversation"
  | "preferences:context"
  | "preferences:diagnostics"
  | "preferences:about"
  | "providers"
  | "tools"
  | "browser"
  | "terminal"
  | "models"
  | "insights"
  | "evolution"
  | "memory";

// 保留旧 id 兼容渐进迁移
export type LegacyNavId =
  | "memory"
  | "tools"
  | "evolution"
  | "insights"
  | "cron"
  | "providers"
  | "models";

export type Tone =
  | "blue"
  | "green"
  | "purple"
  | "cyan"
  | "orange"
  | "pink"
  | "indigo"
  | "amber"
  | "teal"
  | "aurora"
  | "twilight";

type IconComp = ComponentType<SVGProps<SVGSVGElement>>;

export const NAV: {
  id: NavId;
  labelKey: MessageKey;
  Icon: IconComp;
  tone: Tone;
}[] = [
  { id: "chat", labelKey: "nav.chat", Icon: IconChat, tone: "blue" },
  { id: "cron", labelKey: "nav.cron", Icon: IconCron, tone: "cyan" },
  { id: "loop", labelKey: "nav.loop", Icon: IconLoop, tone: "pink" },
  { id: "skills", labelKey: "nav.skills", Icon: IconPlugin, tone: "indigo" },
  {
    id: "settings",
    labelKey: "nav.settings",
    Icon: IconSettings,
    tone: "twilight",
  },
];

export const PAGE_META: Record<
  NavId,
  { titleKey: MessageKey; subKey: MessageKey }
> = {
  chat: { titleKey: "page.chat.title", subKey: "page.chat.sub" },
  cron: { titleKey: "page.cron.title", subKey: "page.cron.sub" },
  loop: { titleKey: "page.loop.title", subKey: "page.loop.sub" },
  skills: { titleKey: "page.skills.title", subKey: "page.skills.sub" },
  settings: { titleKey: "page.settings.title", subKey: "page.settings.sub" },
};
