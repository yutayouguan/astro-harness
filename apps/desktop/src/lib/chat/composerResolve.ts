/**
 * 发送前解析 `/技能` 与 `@提及`：
 * - `/skill`：解析出待加载技能名，交给后端在回合开始前 eager 加载并注入
 * - 可链式多个前导 `/skill`（最多 5 个），其后为用户指令
 * - `@Agent`：切换全局活跃 Agent（本轮起用新 session 绑定）
 * - `@Skill`：同 `/skill`，只传技能名，不再把 SKILL.md 全文拼进用户消息
 * - `@MCP`：启用对应 MCP server（写入全局 `.astro/config.toml`）
 */

import { parseSlashInput, resolveBuiltinSlash } from "./composerCommands.ts";

const MAX_LEADING_SKILLS = 5;

export type MentionCatalog = {
  agents: { id: string; name: string }[];
  skills: { id: string; name: string }[];
  mcpServers: { id: string; name: string }[];
};

export type ResolvedComposerTurn = {
  /** 气泡展示用（去掉前导 /skill，保留可读指令） */
  displayText: string;
  /** 发给模型的正文（干净的用户指令，不含技能全文） */
  modelText: string;
  /** 需要切换到的 Agent id */
  switchAgentId?: string;
  switchAgentName?: string;
  /** 需要启用的 MCP server id */
  enableMcpIds: string[];
  enableMcpNames: string[];
  /** 已加载的技能名 */
  loadedSkills: string[];
};

function norm(s: string): string {
  return s.trim().toLowerCase();
}

function findByName<T extends { id: string; name: string }>(
  list: T[],
  name: string,
): T | undefined {
  const n = norm(name);
  return list.find((x) => norm(x.name) === n || norm(x.id) === n);
}

/**
 * 拆出前导技能斜杠（Hermes：连续 `/skill`，遇非技能名即停）。
 * 内置命令不算技能。
 */
export function peelLeadingSkillSlashes(
  text: string,
  skillNames: string[],
): { skills: string[]; rest: string } {
  const skillSet = new Set(skillNames.map(norm));
  const tokens = text.trim().split(/\s+/);
  const skills: string[] = [];
  let i = 0;
  while (i < tokens.length && skills.length < MAX_LEADING_SKILLS) {
    const tok = tokens[i];
    if (!tok.startsWith("/") || tok.length < 2) break;
    const name = tok.slice(1);
    if (resolveBuiltinSlash(name)) break;
    if (!skillSet.has(norm(name))) break;
    // 还原原始大小写名
    const original = skillNames.find((s) => norm(s) === norm(name)) ?? name;
    skills.push(original);
    i += 1;
  }
  return { skills, rest: tokens.slice(i).join(" ").trim() };
}

/** 提取 `@Name`（字母数字 _ - .）并返回剩余文本 */
export function peelAtMentions(
  text: string,
  catalog: MentionCatalog,
): {
  agents: { id: string; name: string }[];
  skills: { id: string; name: string }[];
  mcps: { id: string; name: string }[];
  rest: string;
} {
  const agents: { id: string; name: string }[] = [];
  const skills: { id: string; name: string }[] = [];
  const mcps: { id: string; name: string }[] = [];
  const seen = new Set<string>();

  const rest = text.replace(/@([^\s@]+)/g, (full, raw: string) => {
    const key = norm(raw);
    if (seen.has(`a:${key}`)) return "";
    const agent = findByName(catalog.agents, raw);
    if (agent) {
      seen.add(`a:${key}`);
      agents.push(agent);
      return "";
    }
    const skill = findByName(catalog.skills, raw);
    if (skill) {
      seen.add(`s:${key}`);
      skills.push(skill);
      return "";
    }
    const mcp = findByName(catalog.mcpServers, raw);
    if (mcp) {
      seen.add(`m:${key}`);
      mcps.push(mcp);
      return "";
    }
    return full;
  });

  return {
    agents,
    skills,
    mcps,
    rest: rest
      .replace(/[ \t]{2,}/g, " ")
      .replace(/\n{3,}/g, "\n\n")
      .trim(),
  };
}

/**
 * 计算发给模型的正文：技能全文不再拼接进用户消息，由后端 eager 注入。
 * 仅当用户只 @ 了技能、没有附加指令时，给一个简短的中性提示。
 */
export function resolveModelText(
  instruction: string,
  hasLoadedSkills: boolean,
  rawTrimmed: string,
): string {
  if (instruction.trim()) return instruction;
  if (hasLoadedSkills) {
    return "(The skill is loaded. Ask what the user needs next if the task is unclear.)";
  }
  return rawTrimmed;
}

/**
 * 解析用户输入并加载技能全文。
 * 若整行是内置斜杠命令（非技能），返回 null，由 slash 分发处理。
 */
export async function resolveComposerTurn(
  rawText: string,
  catalog: MentionCatalog,
): Promise<ResolvedComposerTurn | null> {
  const trimmed = rawText.trim();
  if (!trimmed) {
    return {
      displayText: "",
      modelText: "",
      enableMcpIds: [],
      enableMcpNames: [],
      loadedSkills: [],
    };
  }

  // 纯内置斜杠（且非技能）交给 slash handler
  if (trimmed.startsWith("/")) {
    const skillNames = catalog.skills.map((s) => s.name);
    const parsed = parseSlashInput(trimmed, skillNames);
    if (parsed && parsed.action !== "insert_skill") {
      return null;
    }
  }

  const skillNames = catalog.skills.map((s) => s.name);
  const peeledSlash = peelLeadingSkillSlashes(trimmed, skillNames);
  const peeledAt = peelAtMentions(peeledSlash.rest, catalog);

  const skillNameList = [
    ...peeledSlash.skills,
    ...peeledAt.skills.map((s) => s.name),
  ];
  // 去重保序
  const uniqueSkills: string[] = [];
  for (const n of skillNameList) {
    if (!uniqueSkills.some((x) => norm(x) === norm(n))) uniqueSkills.push(n);
  }

  const instruction = peeledAt.rest;
  const displayText =
    uniqueSkills.length > 0
      ? [...uniqueSkills.map((n) => `/${n}`), instruction]
          .filter(Boolean)
          .join(" ")
      : instruction || trimmed;

  const modelText = resolveModelText(
    instruction,
    uniqueSkills.length > 0,
    trimmed,
  );

  const primaryAgent = peeledAt.agents[0];

  return {
    displayText: displayText || trimmed,
    modelText,
    switchAgentId: primaryAgent?.id,
    switchAgentName: primaryAgent?.name,
    enableMcpIds: peeledAt.mcps.map((m) => m.id),
    enableMcpNames: peeledAt.mcps.map((m) => m.name),
    loadedSkills: uniqueSkills.slice(0, MAX_LEADING_SKILLS),
  };
}
