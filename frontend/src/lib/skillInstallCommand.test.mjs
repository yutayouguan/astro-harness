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

test("SkillHub install prompt links to slug detail page", () => {
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
  const cmd = storeInstallCommand(skill);
  assert.match(cmd, /详情：https:\/\/skillhub\.cn\/skills\/demo-skill/);
  assert.doesNotMatch(cmd, /api\.skillhub\.cn/);
});

test("ClawHub detail URL uses homepage or short link", () => {
  const skill = {
    id: "clawhub:outlit-sdk",
    name: "Outlit SDK",
    description: "desc",
    source: "clawhub",
    store: "clawhub",
    installs: 1305,
    install_ref: "clawhub:outlit-sdk",
    homepage: "https://clawhub.ai/s/skills/outlit-sdk",
  };
  assert.equal(
    storeSkillDetailUrl(skill),
    "https://clawhub.ai/s/skills/outlit-sdk",
  );
  assert.match(storeInstallCommand(skill), /clawhub@latest install outlit-sdk/);
});

test("skills.sh detail URL prefers www host", () => {
  const skill = {
    id: "skillsdotsh:vercel-labs/skills/find-skills",
    name: "find-skills",
    description: "x",
    source: "vercel-labs/skills",
    store: "skillsdotsh",
    installs: 1,
    install_ref: "skillsdotsh:vercel-labs/skills/find-skills",
    homepage: "https://skills.sh/vercel-labs/skills/find-skills",
  };
  assert.equal(
    storeSkillDetailUrl(skill),
    "https://www.skills.sh/vercel-labs/skills/find-skills",
  );
});
