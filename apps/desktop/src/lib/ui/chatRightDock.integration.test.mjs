import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const appSource = await readFile(
  new URL("../../App.tsx", import.meta.url),
  "utf8",
);
const workbenchSource = await readFile(
  new URL("../../hooks/chat/useProjectFileWorkbench.ts", import.meta.url),
  "utf8",
);
const rightPanelStyles = await readFile(
  new URL("../../styles/features/chat/right-panel.css", import.meta.url),
  "utf8",
);

test("project files dock starts closed instead of restoring an open state", () => {
  assert.match(
    workbenchSource,
    /const \[panelOpen, setPanelOpenState\] = useState\(false\);/,
  );
  assert.doesNotMatch(workbenchSource, /astro\.projectFiles\.open/);
});

test("chat shell keeps the project files surface mounted for reversible motion", () => {
  assert.match(appSource, /const activeChatRightDock = resolveChatRightDock/);
  assert.match(
    appSource,
    /<ProjectFilesPanel\s+open=\{activeChatRightDock === "project-files"\}/,
  );
  assert.match(
    appSource,
    /activeChatRightDock === "project-files" \? " has-project-files" : ""/,
  );
  assert.doesNotMatch(
    appSource,
    /activeChatRightDock === "project-files" \? \(/,
  );
  assert.match(
    appSource,
    /activeChatRightDock === "side-chat"\s*&&\s*sideSessionId/,
  );
  assert.match(appSource, /activeChatRightDock === "inspector" && \(/);
  assert.doesNotMatch(appSource, /--chat-header-right-offset/);
  assert.doesNotMatch(appSource, /chatRightDockWidth/);
});

test("side chat has a persistent layout slot around its exit lifecycle", () => {
  assert.match(
    appSource,
    /className=\{`side-chat-dock\$\{activeChatRightDock === "side-chat" \? " is-open" : ""\}`\}/,
  );
  assert.match(
    appSource,
    /<div[\s\S]*?className=\{`side-chat-dock[\s\S]*?<AnimatePresence initial=\{false\}>[\s\S]*?<SideChatPanel/,
  );
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

test("the shared inspector body reserves its scrollbar gutter across tabs", () => {
  assert.match(
    rightPanelStyles,
    /\.chat-right-body\s*\{[\s\S]*?overflow:\s*auto;[\s\S]*?scrollbar-gutter:\s*stable;/,
  );
});
