import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const appSource = await readFile(new URL("../../App.tsx", import.meta.url), "utf8");

test("chat shell renders only the resolved right-dock surface", () => {
  assert.match(appSource, /const activeChatRightDock = resolveChatRightDock/);
  assert.match(appSource, /activeChatRightDock === "project-files" \? \(/);
  assert.match(appSource, /activeChatRightDock === "side-chat" && sideSessionId/);
  assert.match(appSource, /activeChatRightDock === "inspector" && \(/);
  assert.doesNotMatch(appSource, /const chatHeaderRightOffset\s*=\s*\([^;]+\)\s*\+/);
});

test("opening either dock entry closes the previously active surface", () => {
  assert.match(
    appSource,
    /const openChatRightDock[\s\S]+?projectFiles\.setPanelOpen\(false\)[\s\S]+?setChatRightOpen\(true\)/,
  );
  assert.match(
    appSource,
    /const toggleProjectFilesDock[\s\S]+?setChatRightOpen\(false\)[\s\S]+?projectFiles\.setPanelOpen\(true\)/,
  );
});
