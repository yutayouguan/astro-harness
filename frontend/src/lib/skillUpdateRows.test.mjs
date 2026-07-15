import assert from "node:assert/strict";
import test from "node:test";
import { filterUpdateRows, mergeUpdateRows } from "./skillUpdateRows.ts";

const pptInstalled = {
  id: "/Users/iswm/.astro/workspace/skills/ppt-generator-skill",
  name: "ppt-generator",
  description: "",
  path: "",
  source_dir: "",
  enabled: true,
};

const pptOrigin = {
  folder: "ppt-generator-skill",
  name: "ppt-generator-skill",
  store: "skillhub",
  install_ref: "skillhub:owner/ppt-generator-skill",
  agent_id: "workspace",
  installed_at: 1,
};

test("folder matches origin when frontmatter name differs", () => {
  const rows = mergeUpdateRows([pptInstalled], [], [pptOrigin], "workspace");
  assert.equal(rows.length, 1);
  assert.equal(rows[0].status, "with_origin");
  assert.equal(rows[0].origin?.folder, "ppt-generator-skill");
});

test("name match links origin when folder differs", () => {
  const rows = mergeUpdateRows(
    [
      {
        id: "/tmp/skills/weather",
        name: "weather",
        description: "",
        path: "",
        source_dir: "",
        enabled: true,
      },
    ],
    [],
    [
      {
        folder: "weather-skill",
        name: "weather",
        store: "clawhub",
        install_ref: "clawhub:weather",
        agent_id: "workspace",
        installed_at: 1,
      },
    ],
    "workspace",
  );
  assert.equal(rows[0].status, "with_origin");
});

test("no matching origin yields no_origin status", () => {
  const rows = mergeUpdateRows(
    [
      {
        id: "/tmp/skills/orphan",
        name: "orphan",
        description: "",
        path: "",
        source_dir: "",
        enabled: true,
      },
    ],
    [],
    [pptOrigin],
    "workspace",
  );
  assert.equal(rows[0].status, "no_origin");
  assert.equal(rows[0].origin, null);
});

test("linked machine skill is included", () => {
  const rows = mergeUpdateRows(
    [],
    [
      {
        id: "/tmp/machine/skills/find-skills",
        name: "find-skills",
        description: "",
        path: "",
        source_dir: "",
        enabled: true,
        linked: true,
      },
    ],
    [
      {
        folder: "find-skills",
        name: "find-skills",
        store: "clawhub",
        install_ref: "clawhub:find-skills",
        agent_id: "workspace",
        installed_at: 1,
      },
    ],
    "workspace",
  );
  assert.equal(rows.length, 1);
  assert.equal(rows[0].status, "with_origin");
});

test("unlinked machine skill is ignored", () => {
  const rows = mergeUpdateRows(
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
    [
      {
        folder: "find-skills",
        name: "find-skills",
        store: "clawhub",
        install_ref: "clawhub:find-skills",
        agent_id: "workspace",
        installed_at: 1,
      },
    ],
    "workspace",
  );
  assert.equal(rows.length, 0);
});

test("origin for another agent is not matched", () => {
  const rows = mergeUpdateRows(
    [pptInstalled],
    [],
    [{ ...pptOrigin, agent_id: "other-agent" }],
    "workspace",
  );
  assert.equal(rows[0].status, "no_origin");
});

test("default agent id normalizes to workspace", () => {
  const rows = mergeUpdateRows(
    [pptInstalled],
    [],
    [{ ...pptOrigin, agent_id: "default" }],
    "workspace",
  );
  assert.equal(rows[0].status, "with_origin");
});

test("default filter with_origin hides no_origin", () => {
  const rows = mergeUpdateRows(
    [pptInstalled, { ...pptInstalled, id: "/tmp/orphan", name: "orphan" }],
    [],
    [pptOrigin],
    "workspace",
  );
  const filtered = filterUpdateRows(rows, "with_origin");
  assert.equal(filtered.length, 1);
  assert.equal(filtered[0].skill.name, "ppt-generator");
});

test("filter no_origin keeps only unmatched rows", () => {
  const rows = mergeUpdateRows(
    [pptInstalled, { ...pptInstalled, id: "/tmp/orphan", name: "orphan" }],
    [],
    [pptOrigin],
    "workspace",
  );
  const filtered = filterUpdateRows(rows, "no_origin");
  assert.equal(filtered.length, 1);
  assert.equal(filtered[0].skill.name, "orphan");
});

test("filter updatable equals with_origin in v1", () => {
  const rows = mergeUpdateRows(
    [pptInstalled, { ...pptInstalled, id: "/tmp/orphan", name: "orphan" }],
    [],
    [pptOrigin],
    "workspace",
  );
  assert.deepEqual(
    filterUpdateRows(rows, "updatable"),
    filterUpdateRows(rows, "with_origin"),
  );
});
