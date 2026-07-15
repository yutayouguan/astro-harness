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
    const ownerSlug = skillhubOwnerSlug(skill);
    const installHint = skill.install_ref || `skillhub:${ownerSlug}`;
    const detailUrl = `https://skillhub.cn/skills/${skillhubDetailSlug(skill)}`;
    // 一键安装由 Astro 走 api.skillhub.cn 文件 API；提示给 Agent 时仍给可操作引用
    return (
      `请帮我安装 SkillHub 技能「${skill.name}」\n` +
      `- 安装引用：${installHint}\n` +
      `- 文档：${SKILLHUB_INSTALL_DOC}\n` +
      `- 详情：${detailUrl}\n`
    );
  }

  if (skill.store === "clawhub") {
    const installHint = skill.install_ref || `clawhub:${skill.name}`;
    const detailUrl =
      skill.homepage ||
      `https://clawhub.ai/s/skills/${skill.id.replace(/^clawhub:/, "")}`;
    return (
      `请帮我安装 ClawHub 技能「${skill.name}」\n` +
      `- 安装命令：npx --yes clawhub@latest install ${installHint.replace(/^clawhub:/, "")}\n` +
      `- 安装引用：${installHint}\n` +
      `- 详情：${detailUrl}\n`
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
    // 勿用 homepage（api.skillhub.cn/...）或 owner/slug：官网详情路由仅为 /skills/:slug
    return `https://skillhub.cn/skills/${skillhubDetailSlug(skill)}`;
  }

  if (skill.store === "clawhub") {
    if (skill.homepage) return skill.homepage;
    const slug = skill.id.replace(/^clawhub:/, "").split("--").pop() || skill.name;
    return `https://clawhub.ai/s/skills/${slug}`;
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
