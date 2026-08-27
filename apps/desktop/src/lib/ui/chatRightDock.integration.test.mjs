import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const appSource = await readFile(new URL("../../App.tsx", import.meta.url), "utf8");

test("chat shell keeps the project files surface mounted for reversible motion", () => {
  assert.match(appSource, /const activeChatRightDock = resolveChatRightDock/);
  assert.match(
    appSource,
    /<ProjectFilesPanel\s+open=\{activeChatRightDock === "project-files"\}/,
  );
  assert.doesNotMatch(appSource, /activeChatRightDock === "project-files" \? \(/);
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

test("the pinned summary entry opens the reorganized three-tab inspector", async () => {
  const panelSource = await readFile(
    new URL("../../components/chat/ChatRightPanel.tsx", import.meta.url),
    "utf8",
  );

  assert.match(appSource, /const toggleChatSummaryDock/);
  assert.match(appSource, /openChatRightDock\("summary"\)/);
  assert.match(appSource, /header-summary-btn/);
  assert.match(
    panelSource,
    /export type ChatRightTab = "summary" \| "context" \| "branches"/,
  );
  assert.doesNotMatch(panelSource, /const tabs:[^;]+\["agent", "monitor"/);
});
