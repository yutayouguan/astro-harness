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

test("browser dock keeps an exit lifecycle and uses the project drawer motion", async () => {
  const browserStyles = await readFile(
    new URL("../../styles/features/chat/browser-dock.css", import.meta.url),
    "utf8",
  );

  assert.match(appSource, /const browserDockPresence = useDeferredPresence/);
  assert.match(appSource, /browserDockPresence\.mounted \? \(/);
  assert.match(appSource, /open=\{browserDockPresence\.visible\}/);
  assert.match(
    browserStyles,
    /\.browser-dock\s*\{[\s\S]*?position:\s*absolute;[\s\S]*?inset:\s*0;[\s\S]*?width:\s*100%;[\s\S]*?visibility:\s*hidden;/,
  );
  assert.doesNotMatch(browserStyles, /flex-basis/);
  assert.match(
    browserStyles,
    /\.browser-dock\.is-open\s*\{[\s\S]*?visibility:\s*visible;/,
  );
  assert.match(
    browserStyles,
    /transform 300ms cubic-bezier\(0\.22, 1, 0\.36, 1\)/,
  );
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

test("browser focus mode expands inside the chat canvas and keeps the composer available", async () => {
  const browserDock = await readFile(
    new URL("../../components/chat/BrowserDock.tsx", import.meta.url),
    "utf8",
  );
  const chatView = await readFile(
    new URL("../../components/chat/ChatView.tsx", import.meta.url),
    "utf8",
  );
  const browserStyles = await readFile(
    new URL("../../styles/features/chat/browser-dock.css", import.meta.url),
    "utf8",
  );
  const composerStyles = await readFile(
    new URL("../../styles/features/chat/markdown.css", import.meta.url),
    "utf8",
  );

  assert.doesNotMatch(appSource, /browserExpanded/);
  assert.match(appSource, /has-browser is-browser-expanded/);
  assert.match(appSource, /has-browser-surface/);
  assert.match(appSource, /composerPresentation=\{/);
  assert.match(appSource, /onComposerHeightChange=\{/);
  assert.match(appSource, /onComposerOverlayOpenChange=\{/);
  assert.doesNotMatch(browserDock, /Maximize2/);
  assert.doesNotMatch(browserDock, /Minimize2/);
  assert.match(browserDock, /onTitleMouseDown/);
  assert.match(browserDock, /onTitleDoubleClick/);
  assert.match(chatView, /composerPresentation === "capsule"/);
  assert.match(chatView, /Boolean\(workspaceContent\)/);
  assert.doesNotMatch(chatView, /capsuleComposerExpanded/);
  assert.doesNotMatch(chatView, /input\.includes\("\\n"\)/);
  assert.match(browserStyles, /\.chat-layout-with-right\.is-browser-expanded/);
  assert.match(browserStyles, /\.browser-dock\.is-open \.browser-viewport/);
  assert.match(browserStyles, /--browser-composer-height/);
  assert.match(browserStyles, /\.has-composer-overlay/);
  assert.match(composerStyles, /\.composer-shell\.is-capsule/);
  assert.match(composerStyles, /--composer-capsule-frost/);
  assert.match(composerStyles, /border-radius:\s*999px/);
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
