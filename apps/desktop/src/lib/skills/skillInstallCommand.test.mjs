import assert from "node:assert/strict";
import test from "node:test";
import {
  storeInstallCommand,
  storeSkillDetailUrl,
} from "./skillInstallCommand.ts";

test("SkillHub detail URL uses slug only, not owner/slug", () => {
  const skill = {
    id: "skillhub:user_ec205dbb/web-tools-guide",
    name: "web-tools-guide",
    description: "desc",
    source: "community",
    store: "skillhub",
    installs: 3459,
    install_ref: "skillhub:user_ec205dbb/web-tools-guide",
    homepage: "https://api.skillhub.cn/user_ec205dbb/web-tools-guide",
  };
  assert.equal(
    storeSkillDetailUrl(skill),
    "https://skillhub.cn/skills/web-tools-guide",
  );
});

test("SkillHub install prompt pins Astro installer and target scope", () => {
  const skill = {
    id: "skillhub:user_x/demo-skill",
    name: "demo-skill",
    description: "",
    source: "community",
    store: "skillhub",
    installs: null,
    install_ref: "skillhub:user_x/demo-skill",
    homepage: "https://api.skillhub.cn/user_x/demo-skill",
  };
  const cmd = storeInstallCommand(skill, "project");
  assert.match(cmd, /详情：https:\/\/skillhub\.cn\/skills\/demo-skill/);
  assert.match(cmd, /request_plugin_install/);
  assert.match(cmd, /安装作用域：project/);
  assert.match(cmd, /<当前项目>\/\.astro\/skills/);
  assert.match(cmd, /不要运行 SkillHub CLI/);
  assert.doesNotMatch(cmd, /api\.skillhub\.cn/);
});

test("API-key Skill prompt keeps secrets out of chat and files", () => {
  const cmd = storeInstallCommand({
    id: "skillhub:user_x/keyed-skill",
    name: "keyed-skill",
    description: "",
    source: "community",
    store: "skillhub",
    installs: null,
    install_ref: "skillhub:user_x/keyed-skill",
    homepage: null,
    requires_api_key: true,
  });
  assert.match(cmd, /requires_api_key=true/);
  assert.match(cmd, /不要让我在对话中粘贴密钥/);
  assert.match(cmd, /不要把密钥写入 Skill 或项目文件/);
});
