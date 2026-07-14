import { test } from "node:test";
import assert from "node:assert/strict";
import {
  formatSkillInjection,
  peelAtMentions,
  peelLeadingSkillSlashes,
} from "./composerResolve.ts";

test("peels leading skill slashes and keeps the rest", () => {
  const r = peelLeadingSkillSlashes(
    "/alpha /beta fix issue #1",
    ["alpha", "beta", "gamma"],
  );
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

test("formats skill injection like Hermes user message", () => {
  const out = formatSkillInjection(
    [{ name: "demo", content: "# Demo\nDo X" }],
    "run it",
  );
  assert.match(out, /Please follow this skill \(demo\):/);
  assert.match(out, /# Demo\nDo X/);
  assert.match(out, /User request:\nrun it/);
});
