import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [app, sessionList, sessionActionsMenu, plusMenu, chatView] =
  await Promise.all([
    readFile(new URL("../../App.tsx", import.meta.url), "utf8"),
    readFile(
      new URL("../../components/chat/SidebarSessionList.tsx", import.meta.url),
      "utf8",
    ),
    readFile(
      new URL("../../components/chat/SessionActionsMenu.tsx", import.meta.url),
      "utf8",
    ),
    readFile(
      new URL("../../components/chat/ComposerPlusMenu.tsx", import.meta.url),
      "utf8",
    ),
    readFile(
      new URL("../../components/chat/ChatView.tsx", import.meta.url),
      "utf8",
    ),
  ]);

test("session menu moves an idle conversation through the canonical project API", () => {
  assert.match(sessionActionsMenu, /sessions\.moveToProject/);
  assert.match(sessionActionsMenu, /assign_session_to_project/);
  assert.match(
    sessionActionsMenu,
    /status === "running" \|\| status === "awaiting"/,
  );
  assert.match(sessionActionsMenu, /onClearDeletedCurrentSession/);
});

test("title and sidebar entry points share the canonical session actions menu", () => {
  assert.match(app, /<SessionActionsMenu/);
  assert.match(sessionList, /<SessionActionsMenu/);
  assert.doesNotMatch(app, /className="conversation-menu-popover"/);
  for (const action of [
    "sessions.pin",
    "sessions.rename",
    "sessions.regenerateTitle",
    "sessions.export",
    "sessions.branch",
    "sessions.moveToProject",
    "sessions.archive",
    "sessions.deletePermanently",
  ]) {
    assert.match(sessionActionsMenu, new RegExp(action.replace(".", "\\.")));
  }
});

test("composer plus menu can select a folder without recursively uploading it", () => {
  assert.match(plusMenu, /onAttachFolder/);
  assert.match(plusMenu, /chat\.plusMenuFolder/);
  assert.match(chatView, /directory: true/);
  assert.match(chatView, /mime: "inode\/directory"/);
  assert.match(chatView, /kind: "folder"/);
});
