import assert from "node:assert/strict";
import test from "node:test";
import {
  collectInstalledSkillKeys,
  inferFolderFromInstallRef,
  isStoreSkillInstalled,
} from "./skillInstalledMatch.ts";

test("name-only match still works", () => {
  const keys = collectInstalledSkillKeys([
    {
      id: "/tmp/skills/weather",
      name: "weather",
      description: "",
      path: "",
      source_dir: "",
      enabled: true,
    },
  ]);
  assert.equal(
    isStoreSkillInstalled(
      {
        id: "skillhub:owner/weather",
        name: "weather",
        description: "",
        source: "owner",
        store: "skillhub",
        installs: null,
        install_ref: "skillhub:owner/weather",
        homepage: null,
      },
      keys,
    ),
    true,
  );
});

test("store display name vs SKILL.md name: match by folder", () => {
  // 真实案例：商店名 ppt-generator-skill，frontmatter name 为 ppt-generator
  const keys = collectInstalledSkillKeys([
    {
      id: "/Users/iswm/.astro/workspace/skills/ppt-generator-skill",
      name: "ppt-generator",
      description: "",
      path: "",
      source_dir: "",
      enabled: true,
    },
  ]);
  assert.equal(
    isStoreSkillInstalled(
      {
        id: "skillhub:owner/ppt-generator-skill",
        name: "ppt-generator-skill",
        description: "",
        source: "skillhub",
        store: "skillhub",
        installs: 30800,
        install_ref: "skillhub:owner/ppt-generator-skill",
        homepage: null,
      },
      keys,
    ),
    true,
  );
});

test("SkillHub display_name matches via install_ref slug", () => {
  const keys = collectInstalledSkillKeys([
    {
      id: "/tmp/skills/outlit-sdk",
      name: "outlit-sdk",
      description: "",
      path: "",
      source_dir: "",
      enabled: true,
    },
  ]);
  assert.equal(
    isStoreSkillInstalled(
      {
        id: "skillhub:owner/outlit-sdk",
        name: "Outlit SDK",
        description: "",
        source: "owner",
        store: "skillhub",
        installs: null,
        install_ref: "skillhub:owner/outlit-sdk",
        homepage: null,
      },
      keys,
    ),
    true,
  );
});

test("non-SkillHub refs do not infer install folders", () => {
  assert.equal(inferFolderFromInstallRef("legacy:weather"), undefined);
  assert.equal(
    inferFolderFromInstallRef("https://legacy.example/owner/weather"),
    undefined,
  );
});

test("unlinked machine skill is ignored", () => {
  const keys = collectInstalledSkillKeys(
    [],
    [
      {
        id: "/tmp/machine/skills/find-skills",
        name: "find-skills",
        description: "",
        path: "",
        source_dir: "",
        enabled: false,
        linked: false,
      },
    ],
  );
  assert.equal(
    isStoreSkillInstalled(
      {
        id: "skillhub:owner/find-skills",
        name: "find-skills",
        description: "",
        source: "owner",
        store: "skillhub",
        installs: null,
        install_ref: "skillhub:owner/find-skills",
        homepage: null,
      },
      keys,
    ),
    false,
  );
});
