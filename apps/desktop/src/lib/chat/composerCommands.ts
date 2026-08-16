/**
 * Composer `/` 与 `@` 命令注册（参考 Hermes COMMAND_REGISTRY 形态）。
 * 内置命令 + 动态技能/Agent/MCP；发送时拦截执行，不全量照搬 Hermes 网关命令。
 */

import type { MessageKey } from "../../i18n/messages";

/** 斜杠命令执行动作（由 App / ChatView 接线） */
export type SlashAction =
  | "new_chat"
  | "help"
  | "undo"
  | "retry"
  | "stop"
  | "status"
  | "usage"
  | "model"
  | "verbose"
  | "reasoning"
  | "mode"
  | "nav_tools"
  | "nav_skills"
  | "nav_mcp"
  | "nav_memory"
  | "memory_list"
  | "memory_approve"
  | "memory_reject"
  | "memory_refresh"
  | "memory_help"
  | "nav_insights"
  | "nav_providers"
  | "nav_settings"
  | "open_context"
  | "compact"
  | "insert_skill";

/** 内置斜杠命令定义 */
export type BuiltinSlashCommand = {
  /** 主命令名（不含 /） */
  name: string;
  aliases?: string[];
  /** i18n 描述 key */
  descKey: MessageKey;
  action: SlashAction;
  /** 调色板图标 */
  icon: string;
};

/** 解析后的斜杠调用 */
export type ParsedSlash = {
  name: string;
  args: string;
  action: SlashAction;
  /** insert_skill 时为技能名 */
  skillName?: string;
};

/** 提及项类别（Hermes 式分组） */
export type MentionKind = "agent" | "skill" | "mcp";

export type MentionCandidate = {
  kind: MentionKind;
  id: string;
  name: string;
  description?: string;
};

/** Hermes 对齐的内置斜杠命令表 */
export const BUILTIN_SLASH_COMMANDS: BuiltinSlashCommand[] = [
  {
    name: "new",
    aliases: ["reset"],
    descKey: "chat.slashNew",
    action: "new_chat",
    icon: "+",
  },
  {
    name: "clear",
    descKey: "chat.slashClear",
    action: "new_chat",
    icon: "⌫",
  },
  {
    name: "help",
    aliases: ["commands"],
    descKey: "chat.slashHelp",
    action: "help",
    icon: "?",
  },
  {
    name: "undo",
    descKey: "chat.slashUndo",
    action: "undo",
    icon: "↶",
  },
  {
    name: "retry",
    aliases: ["regenerate"],
    descKey: "chat.slashRetry",
    action: "retry",
    icon: "↻",
  },
  {
    name: "stop",
    aliases: ["cancel"],
    descKey: "chat.slashStop",
    action: "stop",
    icon: "■",
  },
  {
    name: "status",
    descKey: "chat.slashStatus",
    action: "status",
    icon: "ℹ",
  },
  {
    name: "usage",
    descKey: "chat.slashUsage",
    action: "usage",
    icon: "∑",
  },
  {
    name: "model",
    descKey: "chat.slashModel",
    action: "model",
    icon: "◈",
  },
  {
    name: "verbose",
    aliases: ["verbosity"],
    descKey: "chat.slashVerbose",
    action: "verbose",
    icon: "☰",
  },
  {
    name: "reasoning",
    aliases: ["think", "thinking"],
    descKey: "chat.slashReasoning",
    action: "reasoning",
    icon: "◉",
  },
  {
    name: "mode",
    descKey: "chat.slashMode",
    action: "mode",
    icon: "⇄",
  },
  {
    name: "tools",
    descKey: "chat.slashTools",
    action: "nav_tools",
    icon: "⚒",
  },
  {
    name: "skills",
    descKey: "chat.slashSkills",
    action: "nav_skills",
    icon: "✦",
  },
  {
    name: "mcp",
    descKey: "chat.slashMcp",
    action: "nav_mcp",
    icon: "⬡",
  },
  {
    name: "memory",
    descKey: "chat.slashMemory",
    action: "nav_memory",
    icon: "▤",
  },
  {
    name: "insights",
    descKey: "chat.slashInsights",
    action: "nav_insights",
    icon: "◔",
  },
  {
    name: "providers",
    descKey: "chat.slashProviders",
    action: "nav_providers",
    icon: "☁",
  },
  {
    name: "settings",
    aliases: ["prefs", "config"],
    descKey: "chat.slashSettings",
    action: "nav_settings",
    icon: "⚙",
  },
  {
    name: "context",
    descKey: "chat.slashContext",
    action: "open_context",
    icon: "▣",
  },
  {
    name: "compact",
    aliases: ["compress"],
    descKey: "chat.slashCompact",
    action: "compact",
    icon: "↯",
  },
];

function normalizeName(raw: string): string {
  return raw.trim().toLowerCase().replace(/^\/+/, "");
}

/** 按精确名或前缀解析内置命令（Hermes 式前缀匹配） */
export function resolveBuiltinSlash(name: string): BuiltinSlashCommand | null {
  const n = normalizeName(name);
  if (!n) return null;

  const exact = BUILTIN_SLASH_COMMANDS.find(
    (c) => c.name === n || c.aliases?.includes(n),
  );
  if (exact) return exact;

  const prefixHits = BUILTIN_SLASH_COMMANDS.filter(
    (c) =>
      c.name.startsWith(n) || c.aliases?.some((a) => a.startsWith(n)),
  );
  return prefixHits[0] ?? null;
}

/**
 * 解析整行输入是否为斜杠命令。
 * 技能名优先于前缀模糊匹配，避免 `/skill` 抢技能。
 */
export function parseSlashInput(
  text: string,
  skillNames: string[] = [],
): ParsedSlash | null {
  const trimmed = text.trim();
  if (!trimmed.startsWith("/")) return null;
  const body = trimmed.slice(1);
  if (!body) return null;

  const sp = body.search(/\s/);
  const rawName = sp >= 0 ? body.slice(0, sp) : body;
  const args = sp >= 0 ? body.slice(sp + 1).trim() : "";
  const name = normalizeName(rawName);
  if (!name) return null;

  const skillHit = skillNames.find((s) => s.toLowerCase() === name);
  if (skillHit) {
    return {
      name,
      args,
      action: "insert_skill",
      skillName: skillHit,
    };
  }

  const builtin = resolveBuiltinSlash(name);
  if (!builtin) return null;

  // `/memory <sub>` 子命令；裸 `/memory` 仍导航记忆页
  if (builtin.name === "memory" && args) {
    const sp = args.search(/\s/);
    const sub = (sp >= 0 ? args.slice(0, sp) : args).toLowerCase();
    const rest = sp >= 0 ? args.slice(sp + 1).trim() : "";
    switch (sub) {
      case "list":
        return { name: "memory", args: rest, action: "memory_list" };
      case "approve":
        return { name: "memory", args: rest, action: "memory_approve" };
      case "reject":
        return { name: "memory", args: rest, action: "memory_reject" };
      case "refresh":
        return { name: "memory", args: "", action: "memory_refresh" };
      case "help":
        return { name: "memory", args: "", action: "memory_help" };
      default:
        return { name: "memory", args, action: "memory_help" };
    }
  }

  return { name: builtin.name, args, action: builtin.action };
}

/** 调色板用：内置命令 + 技能斜杠项（带分组） */
export function buildSlashPaletteEntries(
  skills: { id: string; name: string; description?: string }[],
  t?: (key: string) => string,
) {
  const groupCommands = t?.("chat.paletteGroupCommands") ?? "指令";
  const groupSkills = t?.("chat.paletteGroupSkills") ?? "技能";

  const builtins = BUILTIN_SLASH_COMMANDS.map((c) => ({
    id: `slash-${c.name}`,
    title: `/${c.name}`,
    descKey: c.descKey,
    description: undefined as string | undefined,
    action: c.action,
    icon: c.icon,
    skillName: undefined as string | undefined,
    group: groupCommands,
  }));

  const skillEntries = skills.map((s) => ({
    id: `slash-skill-${s.id}`,
    title: `/${s.name}`,
    descKey: "chat.slashSkill" as MessageKey,
    description: s.description,
    action: "insert_skill" as SlashAction,
    icon: "✦",
    skillName: s.name,
    group: groupSkills,
  }));

  return [...builtins, ...skillEntries];
}

/** @ 提及候选：Agent → Skill → MCP（带分组） */
export function buildMentionCandidates(
  opts: {
    agents: { id: string; name: string }[];
    skills: { id: string; name: string; description?: string }[];
    mcpServers?: { id: string; name: string; description?: string }[];
  },
  t?: (key: string) => string,
): (MentionCandidate & { group?: string })[] {
  const groupAdd = t?.("chat.paletteGroupAdd") ?? "添加";
  const groupPlugins = t?.("chat.paletteGroupPlugins") ?? "插件";

  const agents = opts.agents.map((a) => ({
    kind: "agent" as MentionKind,
    id: a.id,
    name: a.name,
    group: groupAdd,
  }));
  const skills = opts.skills.map((s) => ({
    kind: "skill" as MentionKind,
    id: s.id,
    name: s.name,
    description: s.description,
    group: groupPlugins,
  }));
  const mcps = (opts.mcpServers ?? []).map((m) => ({
    kind: "mcp" as MentionKind,
    id: m.id,
    name: m.name,
    description: m.description,
    group: groupPlugins,
  }));
  return [...agents, ...skills, ...mcps];
}
