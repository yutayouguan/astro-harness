/** 商店 Skill 与本机已安装列表的匹配（名称 / 目录 / slug 可能不一致）。 */
import type { InstalledSkill, StoreSkill } from "../../types";

function addKey(keys: Set<string>, value: string | undefined | null) {
  const v = value?.trim().toLowerCase();
  if (v) keys.add(v);
}

/** 从路径 id 取文件夹名（安装目录名常比 frontmatter name 更接近商店 slug） */
function folderFromId(id: string): string | undefined {
  const parts = id.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1];
}

/** 从 install_ref 推断本地技能文件夹名（与 Rust `infer_folder` 对齐）。 */
export function inferFolderFromInstallRef(installRef: string): string | undefined {
  const trimmed = installRef.trim();
  if (!trimmed) return undefined;
  if (trimmed.startsWith("skillhub:")) {
    const slug = trimmed
      .slice("skillhub:".length)
      .split("/")
      .filter(Boolean)
      .pop();
    return slug?.trim() || undefined;
  }
  if (trimmed.includes("skillhub.cn/")) {
    return trimmed.split("/").filter(Boolean).pop()?.trim() || undefined;
  }
  return undefined;
}

/** 从 SkillHub `owner/slug` 提取可比对的短名。 */
function slugCandidates(raw: string): string[] {
  const trimmed = raw.trim();
  if (!trimmed) return [];
  const withoutScheme = trimmed.includes(":")
    ? trimmed.slice(trimmed.indexOf(":") + 1)
    : trimmed;
  const out: string[] = [];
  const pathTail = withoutScheme.split("/").filter(Boolean).pop();
  if (pathTail) out.push(pathTail);
  out.push(withoutScheme);
  return out;
}

/** 汇总当前 Agent 可用技能的匹配键（已安装 + 已链接本机） */
export function collectInstalledSkillKeys(
  installed: InstalledSkill[],
  machineSkills: InstalledSkill[] = [],
): Set<string> {
  const keys = new Set<string>();
  for (const skill of installed) {
    addKey(keys, skill.name);
    addKey(keys, folderFromId(skill.id));
  }
  for (const skill of machineSkills) {
    if (!skill.linked) continue;
    addKey(keys, skill.name);
    addKey(keys, folderFromId(skill.id));
  }
  return keys;
}

/** 商店条目用于比对的键：展示名 + id / install_ref 的 slug */
export function storeSkillMatchKeys(skill: StoreSkill): string[] {
  const keys = new Set<string>();
  addKey(keys, skill.name);
  for (const raw of [skill.id, skill.install_ref]) {
    for (const c of slugCandidates(raw)) addKey(keys, c);
  }
  return [...keys];
}

/** 商店条目是否已对当前 Agent 可用 */
export function isStoreSkillInstalled(
  skill: StoreSkill,
  availableKeys: Set<string>,
): boolean {
  return storeSkillMatchKeys(skill).some((k) => availableKeys.has(k));
}
