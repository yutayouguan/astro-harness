import assert from "node:assert/strict";
import test from "node:test";

import { sidebarSessionPlacement } from "./sidebarSessionPlacement.ts";

test("pinned placement wins without changing project ownership", () => {
  assert.equal(
    sidebarSessionPlacement({
      pinnedAt: "2026-08-31T00:00:00Z",
      source: "tauri",
      projectId: "project-a",
    }),
    "pinned",
  );
});

test("cron placement wins over its execution project", () => {
  assert.equal(
    sidebarSessionPlacement({
      pinnedAt: null,
      source: "cron",
      projectId: "default",
    }),
    "automation",
  );
});

test("manual project and unassigned sessions have distinct placements", () => {
  assert.equal(
    sidebarSessionPlacement({
      pinnedAt: null,
      source: "tauri",
      projectId: "project-a",
    }),
    "project",
  );
  assert.equal(
    sidebarSessionPlacement({
      pinnedAt: null,
      source: "tauri",
      projectId: null,
    }),
    "recent",
  );
});
