/**
 * 插画注册表：空状态场景 + Agent 封面池。
 */
import type { ComponentType } from "react";
import {
  IllustCoverAssistant,
  IllustCoverChat,
  IllustCoverCode,
  IllustCoverCreative,
  IllustCoverData,
  IllustCoverExplore,
  IllustCoverMusic,
  IllustCoverResearch,
  IllustCoverSchedule,
  IllustCoverShield,
  IllustCoverStars,
  IllustCoverToolkit,
  IllustCoverWriting,
  IllustEmptyChat,
  IllustEmptyCron,
  IllustEmptyFiles,
  IllustEmptyMemory,
  IllustEmptyProviders,
  IllustEmptySkills,
  IllustEmptyLoop,
  IllustEmptyWorkspace,
  type IllustProps,
} from "./art";

export type EmptyScene =
  | "chat"
  | "workspace"
  | "memory"
  | "cron"
  | "files"
  | "providers"
  | "skills"
  | "loop";

export type CoverId =
  | "assistant"
  | "code"
  | "research"
  | "writing"
  | "schedule"
  | "data"
  | "creative"
  | "explore"
  | "shield"
  | "chat"
  | "music"
  | "stars"
  | "toolkit";

export type CoverTone =
  | "blue"
  | "purple"
  | "cyan"
  | "orange"
  | "green"
  | "rose"
  | "amber"
  | "indigo";

export type CoverMeta = {
  id: CoverId;
  labelZh: string;
  labelEn: string;
  tone: CoverTone;
  Art: ComponentType<IllustProps>;
};

export const EMPTY_SCENE_ART: Record<EmptyScene, ComponentType<IllustProps>> = {
  chat: IllustEmptyChat,
  workspace: IllustEmptyWorkspace,
  memory: IllustEmptyMemory,
  cron: IllustEmptyCron,
  files: IllustEmptyFiles,
  providers: IllustEmptyProviders,
  skills: IllustEmptySkills,
  loop: IllustEmptyLoop,
};

export const AGENT_COVERS: CoverMeta[] = [
  {
    id: "assistant",
    labelZh: "助手",
    labelEn: "Assistant",
    tone: "blue",
    Art: IllustCoverAssistant,
  },
  {
    id: "code",
    labelZh: "代码",
    labelEn: "Code",
    tone: "indigo",
    Art: IllustCoverCode,
  },
  {
    id: "research",
    labelZh: "研究",
    labelEn: "Research",
    tone: "cyan",
    Art: IllustCoverResearch,
  },
  {
    id: "writing",
    labelZh: "写作",
    labelEn: "Writing",
    tone: "purple",
    Art: IllustCoverWriting,
  },
  {
    id: "schedule",
    labelZh: "日程",
    labelEn: "Schedule",
    tone: "orange",
    Art: IllustCoverSchedule,
  },
  {
    id: "data",
    labelZh: "数据",
    labelEn: "Data",
    tone: "green",
    Art: IllustCoverData,
  },
  {
    id: "creative",
    labelZh: "创意",
    labelEn: "Creative",
    tone: "amber",
    Art: IllustCoverCreative,
  },
  {
    id: "explore",
    labelZh: "探索",
    labelEn: "Explore",
    tone: "cyan",
    Art: IllustCoverExplore,
  },
  {
    id: "shield",
    labelZh: "安全",
    labelEn: "Security",
    tone: "green",
    Art: IllustCoverShield,
  },
  {
    id: "chat",
    labelZh: "对话",
    labelEn: "Chat",
    tone: "blue",
    Art: IllustCoverChat,
  },
  {
    id: "music",
    labelZh: "音乐",
    labelEn: "Music",
    tone: "rose",
    Art: IllustCoverMusic,
  },
  {
    id: "stars",
    labelZh: "灵感",
    labelEn: "Stars",
    tone: "purple",
    Art: IllustCoverStars,
  },
  {
    id: "toolkit",
    labelZh: "工具",
    labelEn: "Toolkit",
    tone: "amber",
    Art: IllustCoverToolkit,
  },
];

export function getCoverMeta(id: string | null | undefined): CoverMeta | null {
  if (!id) return null;
  return AGENT_COVERS.find((c) => c.id === id) ?? null;
}

/** 封面色板（写入 CSS data-tone） */
export const COVER_TONE_HEX: Record<CoverTone, string> = {
  blue: "#2563eb",
  purple: "#7c3aed",
  cyan: "#0891b2",
  orange: "#ea580c",
  green: "#059669",
  rose: "#e11d48",
  amber: "#d97706",
  indigo: "#4f46e5",
};
