import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [sessionList, plusMenu, chatView] = await Promise.all([
  readFile(new URL("../../components/chat/SidebarSessionList.tsx", import.meta.url), "utf8"),
  readFile(new URL("../../components/chat/ComposerPlusMenu.tsx", import.meta.url), "utf8"),
  readFile(new URL("../../components/chat/ChatView.tsx", import.meta.url), "utf8"),
]);

test("session menu moves an idle conversation through the canonical project API", () => {
  assert.match(sessionList, /sessions\.moveToProject/);
  assert.match(sessionList, /assign_session_to_project/);
  assert.match(sessionList, /runtimeStatus === "running" \|\| runtimeStatus === "awaiting"/);
  assert.match(sessionList, /onClearDeletedCurrentSession/);
});

test("composer plus menu can select a folder without recursively uploading it", () => {
  assert.match(plusMenu, /onAttachFolder/);
  assert.match(plusMenu, /chat\.plusMenuFolder/);
  assert.match(chatView, /directory: true/);
  assert.match(chatView, /mime: "inode\/directory"/);
  assert.match(chatView, /kind: "folder"/);
});
