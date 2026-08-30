import assert from "node:assert/strict";
import test from "node:test";
import {
  applyCheckResults,
  filterUpdateRows,
  mergeUpdateRows,
  summarizeUpdateRows,
} from "./skillUpdateRows.ts";

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
        store: "skillhub",
        install_ref: "skillhub:owner/weather",
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
        store: "skillhub",
        install_ref: "skillhub:owner/find-skills",
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
        store: "skillhub",
        install_ref: "skillhub:owner/find-skills",
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

test("legacy workspace agent id matches canonical default", () => {
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

test("linked machine skill duplicates installed row is collapsed", () => {
  const rows = mergeUpdateRows(
    [pptInstalled],
    [
      {
        id: "/tmp/machine/skills/ppt-generator-skill",
        name: "ppt-generator",
        description: "",
        path: "",
        source_dir: "",
        enabled: true,
        linked: true,
        scope: "machine",
      },
    ],
    [pptOrigin],
    "workspace",
  );
  assert.equal(rows.length, 1);
  assert.equal(rows[0].skill.id, pptInstalled.id);
  assert.equal(rows[0].status, "with_origin");
});

test("applyCheckResults maps outdated status by origin folder", () => {
  const rows = mergeUpdateRows([pptInstalled], [], [pptOrigin], "workspace");
  const applied = applyCheckResults(rows, [
    {
      folder: "ppt-generator-skill",
      status: "outdated",
      remote_version: "2.0.0",
      remote_updated_at: 99,
      message: "",
    },
  ]);
  assert.equal(applied[0].status, "outdated");
});

test("applyCheckResults maps current status by origin folder", () => {
  const rows = mergeUpdateRows([pptInstalled], [], [pptOrigin], "workspace");
  const applied = applyCheckResults(rows, [
    {
      folder: "ppt-generator-skill",
      status: "current",
      remote_version: "1.0.0",
      remote_updated_at: 1,
      message: "",
    },
  ]);
  assert.equal(applied[0].status, "current");
});

test("applyCheckResults leaves no_origin rows unchanged", () => {
  const rows = mergeUpdateRows(
    [{ ...pptInstalled, id: "/tmp/orphan", name: "orphan" }],
    [],
    [pptOrigin],
    "workspace",
  );
  const applied = applyCheckResults(rows, [
    {
      folder: "ppt-generator-skill",
      status: "outdated",
      remote_version: null,
      remote_updated_at: null,
      message: "",
    },
  ]);
  assert.equal(applied[0].status, "no_origin");
});

test("applyCheckResults keeps with_origin when folder has no check", () => {
  const rows = mergeUpdateRows([pptInstalled], [], [pptOrigin], "workspace");
  const applied = applyCheckResults(rows, []);
  assert.equal(applied[0].status, "with_origin");
});

test("filter updatable includes only outdated rows", () => {
  const rows = mergeUpdateRows([pptInstalled], [], [pptOrigin], "workspace");
  const applied = applyCheckResults(rows, [
    {
      folder: "ppt-generator-skill",
      status: "outdated",
      remote_version: "2.0.0",
      remote_updated_at: 99,
      message: "",
    },
  ]);
  const filtered = filterUpdateRows(applied, "updatable");
  assert.equal(filtered.length, 1);
  assert.equal(filtered[0].status, "outdated");
});

test("filter updatable excludes current rows", () => {
  const rows = mergeUpdateRows([pptInstalled], [], [pptOrigin], "workspace");
  const applied = applyCheckResults(rows, [
    {
      folder: "ppt-generator-skill",
      status: "current",
      remote_version: "1.0.0",
      remote_updated_at: 1,
      message: "",
    },
  ]);
  assert.equal(filterUpdateRows(applied, "updatable").length, 0);
});

test("filter updatable excludes unknown rows", () => {
  const rows = mergeUpdateRows([pptInstalled], [], [pptOrigin], "workspace");
  const applied = applyCheckResults(rows, [
    {
      folder: "ppt-generator-skill",
      status: "unknown",
      remote_version: null,
      remote_updated_at: null,
      message: "",
    },
  ]);
  assert.equal(filterUpdateRows(applied, "updatable").length, 0);
});

test("filter updatable excludes machine-scoped rows with origin", () => {
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
        scope: "machine",
      },
    ],
    [
      {
        folder: "find-skills",
        name: "find-skills",
        store: "skillhub",
        install_ref: "skillhub:owner/find-skills",
        agent_id: "workspace",
        installed_at: 1,
      },
    ],
    "workspace",
  );
  assert.equal(filterUpdateRows(rows, "with_origin").length, 1);
  assert.equal(filterUpdateRows(rows, "updatable").length, 0);
});

test("summarizeUpdateRows separates updates, current, attention and missing origins", () => {
  const base = mergeUpdateRows(
    [
      pptInstalled,
      { ...pptInstalled, id: "/tmp/current", name: "current" },
      { ...pptInstalled, id: "/tmp/error", name: "error" },
      { ...pptInstalled, id: "/tmp/orphan", name: "orphan" },
    ],
    [],
    [
      pptOrigin,
      { ...pptOrigin, folder: "current", name: "current" },
      { ...pptOrigin, folder: "error", name: "error" },
    ],
    "workspace",
  );
  const rows = applyCheckResults(base, [
    {
      folder: "ppt-generator-skill",
      status: "outdated",
      remote_version: "2.0.0",
      remote_updated_at: 99,
      message: "",
    },
    {
      folder: "current",
      status: "current",
      remote_version: "1.0.0",
      remote_updated_at: 1,
      message: "",
    },
    {
      folder: "error",
      status: "error",
      remote_version: null,
      remote_updated_at: null,
      message: "network error",
    },
  ]);

  assert.deepEqual(summarizeUpdateRows(rows), {
    total: 4,
    tracked: 3,
    outdated: 1,
    current: 1,
    attention: 1,
    noOrigin: 1,
  });
});
