import type { ComponentType, SVGProps } from "react";
import {
  IconChat,
  IconCron,
  IconFileSpace,
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
  | "workspace"
  | "filespace"
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
  | "teal";

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
    id: "workspace",
    labelKey: "nav.workspace",
    Icon: IconWorkspace,
    tone: "purple",
  },
  {
    id: "filespace",
    labelKey: "nav.filespace",
    Icon: IconFileSpace,
    tone: "cyan",
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
    tone: "amber",
  },
  {
    id: "settings",
    labelKey: "nav.settings",
    Icon: IconSettings,
    tone: "pink",
  },
];

export const PAGE_META: Record<
  NavId,
  { titleKey: MessageKey; subKey: MessageKey }
> = {
  chat: { titleKey: "page.chat.title", subKey: "page.chat.sub" },
  memory: { titleKey: "page.memory.title", subKey: "page.memory.sub" },
  workspace: {
    titleKey: "page.workspace.title",
    subKey: "page.workspace.sub",
  },
  filespace: {
    titleKey: "page.filespace.title",
    subKey: "page.filespace.sub",
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
