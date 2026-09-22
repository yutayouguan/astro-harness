import { test } from "node:test";
import assert from "node:assert/strict";
import {
  peelAtMentions,
  peelLeadingSkillSlashes,
  resolveModelText,
} from "./composerResolve.ts";

test("peels leading skill slashes and keeps the rest", () => {
  const r = peelLeadingSkillSlashes("/alpha /beta fix issue #1", [
    "alpha",
    "beta",
    "gamma",
  ]);
  assert.deepEqual(r.skills, ["alpha", "beta"]);
  assert.equal(r.rest, "fix issue #1");
});

test("stops at builtin slash", () => {
  const r = peelLeadingSkillSlashes("/help me", ["help-me"]);
  assert.deepEqual(r.skills, []);
  assert.equal(r.rest, "/help me");
});

test("peels @agent @skill @mcp", () => {
  const r = peelAtMentions("@Coder use @git-skill with @maps please", {
    agents: [{ id: "a1", name: "Coder" }],
    skills: [{ id: "s1", name: "git-skill" }],
    mcpServers: [{ id: "m1", name: "maps" }],
  });
  assert.deepEqual(
    r.agents.map((a) => a.name),
    ["Coder"],
  );
  assert.deepEqual(
    r.skills.map((s) => s.name),
    ["git-skill"],
  );
  assert.deepEqual(
    r.mcps.map((m) => m.name),
    ["maps"],
  );
  assert.equal(r.rest, "use with please");
});

test("resolves clean model text without inlining skill content", () => {
  // 有指令时只保留用户指令，不再拼进 SKILL.md 全文
  assert.equal(
    resolveModelText("帮我查天气", true, "@aihot 帮我查天气"),
    "帮我查天气",
  );
  // 只 @ 了技能、没有附加指令时给中性提示
  assert.match(resolveModelText("", true, "@aihot"), /skill is loaded/);
  // 无技能提及时原样返回
  assert.equal(resolveModelText("hello", false, "hello"), "hello");
});
