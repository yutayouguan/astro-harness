import type { ComponentType, SVGProps } from "react";
import {
  IconChat,
  IconLoop,
  IconSettings,
  IconSkills,
  IconWorkspace,
} from "../../components/icons";
import type { MessageKey } from "../../i18n/messages";
import { HEADER_AGENT_PICKER_NAV_IDS } from "./headerAgentPicker";

export {
  HEADER_AGENT_PICKER_NAV_IDS,
  showsHeaderAgentPicker,
} from "./headerAgentPicker";

export type NavId =
  | "chat"
  | "loop"
  | "files"
  | "skills"
  | "settings";

// 合并到 settings 面板的二级 tab
export type SettingsTabId =
  | "preferences"
  | "providers"
  | "tools"
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
  { id: "loop", labelKey: "nav.loop", Icon: IconLoop, tone: "pink" },
  { id: "files", labelKey: "nav.files", Icon: IconWorkspace, tone: "purple" },
  { id: "skills", labelKey: "nav.skills", Icon: IconSkills, tone: "indigo" },
  { id: "settings", labelKey: "nav.settings", Icon: IconSettings, tone: "twilight" },
];

export const PAGE_META: Record<
  NavId,
  { titleKey: MessageKey; subKey: MessageKey }
> = {
  chat: { titleKey: "page.chat.title", subKey: "page.chat.sub" },
  loop: { titleKey: "page.loop.title", subKey: "page.loop.sub" },
  files: { titleKey: "page.files.title", subKey: "page.files.sub" },
  skills: { titleKey: "page.skills.title", subKey: "page.skills.sub" },
  settings: { titleKey: "page.settings.title", subKey: "page.settings.sub" },
};

/** 标题栏展示统一 AgentPicker 的导航页（memory 保留页内专用入口）。 */
export const HEADER_AGENT_PICKER_NAVS: ReadonlySet<NavId> = new Set(
  HEADER_AGENT_PICKER_NAV_IDS,
);
