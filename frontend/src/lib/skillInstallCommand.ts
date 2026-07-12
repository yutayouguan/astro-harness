/** Skill 安装命令拼装。 */
import type { StoreSkill } from "../types";

const SKILLHUB_INSTALL_DOC = "https://skillhub.cn/install/skillhub.md";

function parseSkillsDotSh(
  skill: StoreSkill,
): { source: string; skillId: string } | null {
  const fromId = skill.id.match(/^skillsdotsh:(.+)\/([^/]+)$/);
  if (fromId) return { source: fromId[1], skillId: fromId[2] };

  const fromHome = skill.homepage?.match(/skills\.sh\/(.+)\/([^/?#]+)/);
  if (fromHome) return { source: fromHome[1], skillId: fromHome[2] };

  return null;
}

function skillhubSlug(skill: StoreSkill): string {
  const fromId = skill.id.replace(/^skillhub:/, "");
  if (fromId && fromId !== skill.id) return fromId;
  return skill.name;
}

/** 生成可粘贴给 Agent / 终端的安装命令或 Prompt */
export function storeInstallCommand(skill: StoreSkill): string {
  if (skill.store === "skillsdotsh") {
    const parsed = parseSkillsDotSh(skill);
    if (parsed) {
      return `npx skills add ${parsed.source}/${parsed.skillId}`;
    }
    if (skill.install_ref) {
      return `npx skills add ${skill.install_ref}`;
    }
  }

  if (skill.store === "skillhub") {
    const slug = skillhubSlug(skill);
    return (
      `请按 SkillHub 文档安装技能「${skill.name}」\n` +
      `- 安装引用：${skill.install_ref || slug}\n` +
      `- 文档：${SKILLHUB_INSTALL_DOC}\n` +
      (skill.homepage ? `- 主页：${skill.homepage}\n` : "")
    );
  }

  return (
    `请帮我安装这个 Skill：\n` +
    `- 名称：${skill.name}\n` +
    `- 来源：${skill.source}\n` +
    `- 安装引用：${skill.install_ref}\n` +
    (skill.homepage ? `- 主页：${skill.homepage}\n` : "")
  );
}

export function storeSkillDetailUrl(skill: StoreSkill): string | null {
  if (skill.store === "skillsdotsh") {
    const parsed = parseSkillsDotSh(skill);
    if (parsed) {
      return `https://www.skills.sh/${parsed.source}/${parsed.skillId}`;
    }
    if (skill.homepage) {
      return skill.homepage.replace(
        "https://skills.sh/",
        "https://www.skills.sh/",
      );
    }
  }

  if (skill.store === "skillhub") {
    return `https://skillhub.cn/skills/${skillhubSlug(skill)}`;
  }

  return skill.homepage;
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
