import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import type { ProjectDto } from "../../types";
import {
  DEFAULT_PROJECT_PLACEHOLDER,
  ensureDefaultProjectVisible,
  loadProjectsWithRetry,
} from "./projectBootstrap.ts";

const project = (id: string): ProjectDto => ({
  id,
  name: id,
  icon: null,
  roots: [],
  position: 1,
  createdAt: "",
  updatedAt: "",
});

test("default project stays visible while the backend list is unavailable", () => {
  assert.deepEqual(ensureDefaultProjectVisible([]), [
    DEFAULT_PROJECT_PLACEHOLDER,
  ]);
  assert.deepEqual(ensureDefaultProjectVisible([project("custom")]), [
    DEFAULT_PROJECT_PLACEHOLDER,
    project("custom"),
  ]);
});

test("an existing backend default project is preserved without duplication", () => {
  const backendDefault = {
    ...DEFAULT_PROJECT_PLACEHOLDER,
    roots: ["/workspace"],
  };
  const projects = [backendDefault, project("custom")];
  assert.strictEqual(ensureDefaultProjectVisible(projects), projects);
});

test("project loading retries transient failures before returning the list", async () => {
  let calls = 0;
  const waits: number[] = [];
  const projects = await loadProjectsWithRetry(
    async () => {
      calls += 1;
      if (calls < 3) throw new Error("database busy");
      return [project("default")];
    },
    async (delayMs) => {
      waits.push(delayMs);
    },
  );

  assert.equal(calls, 3);
  assert.deepEqual(waits, [250, 1_000]);
  assert.equal(projects[0]?.id, "default");
});

test("an empty backend response is retried and eventually fails closed", async () => {
  let calls = 0;
  await assert.rejects(
    loadProjectsWithRetry(
      async () => {
        calls += 1;
        return [];
      },
      async () => {},
    ),
    /project list is empty/,
  );
  assert.equal(calls, 3);
});

test("App uses the atomic project list command and preserves its fallback", async () => {
  const app = await readFile(new URL("../../App.tsx", import.meta.url), "utf8");
  assert.match(app, /loadProjectsWithRetry\(\(\) =>/);
  assert.match(app, /invoke<ProjectDto\[]>\("list_projects"\)/);
  assert.match(app, /setProjects\(list\);[\s\S]*?dispatchSessionsChanged\(\)/);
  assert.doesNotMatch(app, /invoke<ProjectDto>\("ensure_default_project"\)/);
  assert.doesNotMatch(app, /setProjects\(\[\]\)/);
});
