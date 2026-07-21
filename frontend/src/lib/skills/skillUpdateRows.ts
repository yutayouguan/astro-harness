/** 「更新」Tab：合并已安装技能与来源记录、按筛选过滤。 */
import type {
  InstalledSkill,
  SkillOriginRecord,
  SkillUpdateCheckResult,
  SkillUpdateFilter,
  SkillUpdateRow,
} from "../../types";

/** 从路径 id 取文件夹名（与 `skillInstalledMatch` 一致） */
function folderFromId(id: string): string | undefined {
  const parts = id.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1];
}

function norm(value: string | undefined | null): string | undefined {
  const v = value?.trim().toLowerCase();
  return v || undefined;
}

/** Agent id 规范化（空 / default → workspace，与 memory crate 一致） */
function normalizeAgentId(id: string | null | undefined): string {
  if (!id || id === "workspace") return "default";
  return id;
}

/** 来源记录的 agent_id 规范化（空 / default → workspace） */
function originAgentId(record: SkillOriginRecord): string {
  return normalizeAgentId(record.agent_id);
}

function originMatchesAgent(record: SkillOriginRecord, agentId: string): boolean {
  return originAgentId(record) === normalizeAgentId(agentId);
}

/** origin.folder ↔ skill.id 末段，或 origin.name ↔ skill.name（小写） */
export function originMatchesSkill(
  origin: SkillOriginRecord,
  skill: InstalledSkill,
): boolean {
  const skillFolder = norm(folderFromId(skill.id));
  const originFolder = norm(origin.folder);
  if (skillFolder && originFolder && skillFolder === originFolder) {
    return true;
  }
  const skillName = norm(skill.name);
  const originName = norm(origin.name);
  return Boolean(skillName && originName && skillName === originName);
}

/** 本机链接技能是否已与已安装（astro）条目重复（文件夹末段或名称） */
function machineSkillDuplicatesInstalled(
  machineSkill: InstalledSkill,
  installed: InstalledSkill[],
): boolean {
  const machineFolder = norm(folderFromId(machineSkill.id));
  const machineName = norm(machineSkill.name);
  for (const skill of installed) {
    const folder = norm(folderFromId(skill.id));
    const name = norm(skill.name);
    if (machineFolder && folder && machineFolder === folder) return true;
    if (machineName && name && machineName === name) return true;
  }
  return false;
}

function collectAgentSkills(
  installed: InstalledSkill[],
  linkedMachine: InstalledSkill[],
): InstalledSkill[] {
  const rows: InstalledSkill[] = [...installed];
  for (const skill of linkedMachine) {
    if (!skill.linked) continue;
    if (machineSkillDuplicatesInstalled(skill, installed)) continue;
    rows.push(skill);
  }
  return rows;
}

function findOriginForSkill(
  skill: InstalledSkill,
  origins: SkillOriginRecord[],
): SkillOriginRecord | null {
  for (const origin of origins) {
    if (originMatchesSkill(origin, skill)) return origin;
  }
  return null;
}

/** 合并扫盘列表与当前 Agent 的来源记录 */
export function mergeUpdateRows(
  installed: InstalledSkill[],
  linkedMachine: InstalledSkill[],
  origins: SkillOriginRecord[],
  agentId: string,
): SkillUpdateRow[] {
  const agentOrigins = origins.filter((o) => originMatchesAgent(o, agentId));
  return collectAgentSkills(installed, linkedMachine).map((skill) => {
    const origin = findOriginForSkill(skill, agentOrigins);
    return {
      skill,
      origin,
      status: origin ? "with_origin" : "no_origin",
    };
  });
}

/**
 * v1：`update_installed_skill` 会重装到 Agent 工作区 skills 目录（astro 安装目标）。
 * 纯本机 scope 的技能不在此更新，避免误写到机器路径；请从「已安装」或「更新」Tab 操作。
 */
export function canUpdateSkillFromOrigin(
  skill: InstalledSkill,
  origin: SkillOriginRecord | null,
): boolean {
  if (!origin) return false;
  if (skill.scope === "machine") return false;
  return true;
}

/** 将远端检查结果按 origin.folder 合并到行状态 */
export function applyCheckResults(
  rows: SkillUpdateRow[],
  checks: SkillUpdateCheckResult[],
): SkillUpdateRow[] {
  const byFolder = new Map(checks.map((c) => [c.folder, c]));
  return rows.map((row) => {
    if (!row.origin) return row;
    const check = byFolder.get(row.origin.folder);
    if (!check) return row;
    return { ...row, status: check.status };
  });
}

/** 按筛选芯片过滤合并行；`updatable` 仅保留远端检查为 outdated 的行 */
export function filterUpdateRows(
  rows: SkillUpdateRow[],
  filter: SkillUpdateFilter,
): SkillUpdateRow[] {
  if (filter === "no_origin") {
    return rows.filter((r) => r.status === "no_origin");
  }
  if (filter === "updatable") {
    return rows.filter((r) => r.status === "outdated");
  }
  return rows.filter((r) => r.origin !== null);
}
