/** Skill 安装命令拼装。 */
import type { StoreSkill } from "../../types";

const SKILLHUB_INSTALL_DOC = "https://skillhub.cn/install/skillhub.md";

/** 安装引用用的 owner/slug（来自 id `skillhub:owner/slug`） */
function skillhubOwnerSlug(skill: StoreSkill): string {
  const fromId = skill.id.replace(/^skillhub:/, "");
  if (fromId && fromId !== skill.id) return fromId;
  return skill.name;
}

/** 详情页路径只接受 skills/:slug，不要带 owner（带 owner 会 SPA 404） */
function skillhubDetailSlug(skill: StoreSkill): string {
  const fromId = skill.id.replace(/^skillhub:/, "");
  if (fromId && fromId !== skill.id) {
    const parts = fromId.split("/").filter(Boolean);
    return parts[parts.length - 1] || skill.name;
  }
  return skill.name;
}

/** 生成可粘贴给 Agent / 终端的安装命令或 Prompt */
export function storeInstallCommand(skill: StoreSkill): string {
  const ownerSlug = skillhubOwnerSlug(skill);
  const installHint = skill.install_ref || `skillhub:${ownerSlug}`;
  const detailUrl = `https://skillhub.cn/skills/${skillhubDetailSlug(skill)}`;
  // 一键安装由 Astro 走 api.skillhub.cn 文件 API；提示给 Agent 时仍给可操作引用。
  return (
    `请帮我安装 SkillHub 技能「${skill.name}」\n` +
    `- 安装引用：${installHint}\n` +
    `- 文档：${SKILLHUB_INSTALL_DOC}\n` +
    `- 详情：${detailUrl}\n`
  );
}

export function storeSkillDetailUrl(skill: StoreSkill): string | null {
  // 勿用 homepage（api.skillhub.cn/...）或 owner/slug：官网详情路由仅为 /skills/:slug。
  return `https://skillhub.cn/skills/${skillhubDetailSlug(skill)}`;
}

export function storeCardDescription(
  skill: StoreSkill,
  installsLabel: string,
): string {
  const desc = skill.description?.trim();
  if (desc) return desc;
  if (skill.installs != null) {
    return `${installsLabel}: ${skill.installs}`;
  }
  return skill.source;
}
