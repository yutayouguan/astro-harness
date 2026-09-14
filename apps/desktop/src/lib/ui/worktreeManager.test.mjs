import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const dialog = await readFile(
  new URL("../../components/chat/WorktreeManagerDialog.tsx", import.meta.url),
  "utf8",
);
const app = await readFile(new URL("../../App.tsx", import.meta.url), "utf8");
const parallelTasks = await readFile(
  new URL("../../hooks/chat/useParallelTasks.ts", import.meta.url),
  "utf8",
);

test("project worktree action opens the recovery manager", () => {
  assert.match(app, /setWorktreeProject\(project\)/);
  assert.match(app, /<WorktreeManagerDialog/);
  assert.match(dialog, /"list_task_worktrees"/);
});

test("dirty worktrees cannot be removed and new tasks bind their session", () => {
  assert.match(dialog, /disabled=\{item\.dirty\}/);
  assert.match(dialog, /"cleanup_task_worktree"/);
  assert.match(parallelTasks, /"prepare_task_worktree", \{ sessionId \}/);
  assert.match(parallelTasks, /ownerSessionId: prepared\.ownerSessionId/);
});
