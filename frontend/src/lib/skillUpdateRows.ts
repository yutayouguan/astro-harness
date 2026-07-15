/** 「更新」Tab：合并已安装技能与来源记录、按筛选过滤。 */
import type {
  InstalledSkill,
  SkillOriginRecord,
  SkillUpdateFilter,
  SkillUpdateRow,
} from "../types";

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
  if (!id || id === "default") return "workspace";
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

function collectAgentSkills(
  installed: InstalledSkill[],
  linkedMachine: InstalledSkill[],
): InstalledSkill[] {
  const rows: InstalledSkill[] = [...installed];
  for (const skill of linkedMachine) {
    if (skill.linked) rows.push(skill);
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

/** 按筛选芯片过滤合并行；v1 中 `updatable` 与 `with_origin` 相同 */
export function filterUpdateRows(
  rows: SkillUpdateRow[],
  filter: SkillUpdateFilter,
): SkillUpdateRow[] {
  if (filter === "no_origin") {
    return rows.filter((r) => r.status === "no_origin");
  }
  return rows.filter((r) => r.status === "with_origin");
}
