import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  scopeFallbackSessionsToProject,
  sidebarSessionPlacement,
} from "./sidebarSessionPlacement.ts";
import type { RecentSessionDto } from "../../types";

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

const session = (
  sessionId: string,
  projectId: string | null,
  source = "tauri",
): RecentSessionDto => ({
  sessionId,
  source,
  projectId,
  summary: sessionId,
  createdAt: null,
});

test("fallback session lists recover default and exact custom project membership", () => {
  const sessions = [
    session("default", "default"),
    session("legacy", null),
    session("custom", "project-a"),
    session("cron", null, "cron"),
  ];
  assert.deepEqual(
    scopeFallbackSessionsToProject(sessions, "default").map((item) => [
      item.sessionId,
      item.projectId,
    ]),
    [
      ["default", "default"],
      ["legacy", "default"],
    ],
  );
  assert.deepEqual(
    scopeFallbackSessionsToProject(sessions, "project-a").map(
      (item) => item.sessionId,
    ),
    ["custom"],
  );
});

test("sidebar keeps existing rows and falls back to the stable recent-session command", async () => {
  const source = await readFile(
    new URL("../../components/chat/SidebarSessionList.tsx", import.meta.url),
    "utf8",
  );
  assert.match(source, /"list_recent_sessions"/);
  assert.match(source, /scopeFallbackSessionsToProject/);
  assert.match(source, /listKind !== "active"[\s\S]*?setItems\(\[\]\)/);
});
