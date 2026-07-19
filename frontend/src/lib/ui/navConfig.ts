import type { ComponentType, SVGProps } from "react";
import {
  IconChat,
  IconCron,
  IconInsights,
  IconMemory,
  IconProviders,
  IconSettings,
  IconSkills,
  IconTools,
  IconWorkspace,
} from "../../components/icons";
import type { MessageKey } from "../../i18n/messages";

export type NavId =
  | "chat"
  | "memory"
  | "files"
  | "skills"
  | "tools"
  | "insights"
  | "cron"
  | "providers"
  | "settings";

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
  { id: "memory", labelKey: "nav.memory", Icon: IconMemory, tone: "green" },
  {
    id: "files",
    labelKey: "nav.files",
    Icon: IconWorkspace,
    tone: "purple",
  },
  { id: "skills", labelKey: "nav.skills", Icon: IconSkills, tone: "indigo" },
  { id: "tools", labelKey: "nav.tools", Icon: IconTools, tone: "orange" },
  { id: "cron", labelKey: "nav.cron", Icon: IconCron, tone: "teal" },
  {
    id: "providers",
    labelKey: "nav.providers",
    Icon: IconProviders,
    tone: "blue",
  },
  {
    id: "insights",
    labelKey: "nav.insights",
    Icon: IconInsights,
    tone: "aurora",
  },
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
  memory: { titleKey: "page.memory.title", subKey: "page.memory.sub" },
  files: {
    titleKey: "page.files.title",
    subKey: "page.files.sub",
  },
  skills: { titleKey: "page.skills.title", subKey: "page.skills.sub" },
  tools: { titleKey: "page.tools.title", subKey: "page.tools.sub" },
  insights: { titleKey: "page.insights.title", subKey: "page.insights.sub" },
  cron: { titleKey: "page.cron.title", subKey: "page.cron.sub" },
  providers: {
    titleKey: "page.providers.title",
    subKey: "page.providers.sub",
  },
  settings: { titleKey: "page.settings.title", subKey: "page.settings.sub" },
};
