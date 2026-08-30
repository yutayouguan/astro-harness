/** Skill 安装命令拼装。 */
import type { StoreSkill } from "../../types";

export type SkillInstallTarget = "global" | "project";

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
export function storeInstallCommand(
  skill: StoreSkill,
  target: SkillInstallTarget = "global",
): string {
  const ownerSlug = skillhubOwnerSlug(skill);
  const installHint = skill.install_ref || `skillhub:${ownerSlug}`;
  const detailUrl = `https://skillhub.cn/skills/${skillhubDetailSlug(skill)}`;
  const targetPath =
    target === "project" ? "<当前项目>/.astro/skills" : "~/.astro/skills";
  const apiKeyNote =
    skill.requires_api_key === true
      ? "- 凭据：安装后读取 SKILL.md 确认准确的 API Key 名称；不要让我在对话中粘贴密钥，也不要把密钥写入 Skill 或项目文件。\n"
      : "";
  return (
    `请通过 Astro 内置的 request_plugin_install 工具安装 SkillHub 技能「${skill.name}」，不要运行 SkillHub CLI，也不要安装到 .agents、.codex 或 ./skills。\n` +
    `- 安装引用：${installHint}\n` +
    `- 安装作用域：${target}\n` +
    `- 目标目录：${targetPath}\n` +
    `- 工具参数：skill_id=${ownerSlug}，install_ref=${installHint}，scope=${target}，requires_api_key=${skill.requires_api_key === true}\n` +
    apiKeyNote +
    `- 详情：${detailUrl}\n`
  );
}

/** 安装完成后，让 Agent 只检查凭据要求，不在对话或文件中收集密钥。 */
export function storeConfigureCommand(
  skill: StoreSkill,
  target: SkillInstallTarget,
): string {
  const targetPath =
    target === "project" ? "<当前项目>/.astro/skills" : "~/.astro/skills";
  return (
    `SkillHub 技能「${skill.name}」已经安装到 ${targetPath}。\n` +
    "请读取已安装的 SKILL.md，确认它实际需要的凭据名称、申请地址和配置方式，并用简短步骤告诉我。\n" +
    "不要要求我在对话中粘贴 API Key，不要把密钥写入 Skill、项目文件或日志；如果当前 Astro 没有对应的安全凭据入口，请明确说明，不要假装已经配置完成。\n"
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
