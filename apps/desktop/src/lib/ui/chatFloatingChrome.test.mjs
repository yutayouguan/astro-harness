import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const headerStyles = await readFile(
  new URL("../../styles/features/shell/header.css", import.meta.url),
  "utf8",
);
const chatStyles = await readFile(
  new URL("../../styles/features/chat/markdown.css", import.meta.url),
  "utf8",
);
const coreStyles = await readFile(
  new URL("../../styles/features/chat/core.css", import.meta.url),
  "utf8",
);
const rightPanelStyles = await readFile(
  new URL("../../styles/features/chat/right-panel.css", import.meta.url),
  "utf8",
);
const projectFilesStyles = await readFile(
  new URL("../../styles/features/chat/project-files.css", import.meta.url),
  "utf8",
);
const sideChatStyles = await readFile(
  new URL("../../styles/features/chat/side-chat.css", import.meta.url),
  "utf8",
);
const chatView = await readFile(
  new URL("../../components/chat/ChatView.tsx", import.meta.url),
  "utf8",
);

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(
    new RegExp(`(?:^|\\n)${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\}`),
  )?.groups?.body;
}

test("chat header reserves a dock-aware lane for floating controls", () => {
  const header = rule(headerStyles, ".content-header--chat");
  const actions = rule(headerStyles, ".content-header--chat .header-actions");

  assert.ok(header, "missing chat header rule");
  assert.match(header, /min-height:\s*60px;/);
  assert.ok(actions, "missing chat header action rule");
  assert.match(actions, /position:\s*absolute;/);
  assert.match(actions, /right:\s*calc\(16px \+ var\(--chat-header-right-offset, 0px\)\);/);
  assert.match(actions, /bottom:\s*8px;/);
});

test("right-side chat surfaces dock to the bottom and right edges", () => {
  const layout = rule(rightPanelStyles, ".chat-layout-with-right.has-right-dock");
  const runtimePanel = rule(rightPanelStyles, ".chat-right-panel");
  const projectPanel = rule(projectFilesStyles, ".project-files-panel");
  const projectWorkbench = rule(projectFilesStyles, ".project-file-workbench");
  const sidePanel = rule(sideChatStyles, ".side-chat-panel");

  assert.ok(layout, "missing docked chat layout rule");
  assert.match(layout, /border-radius:\s*28px 0 0 0;/);
  assert.ok(runtimePanel, "missing runtime panel rule");
  assert.match(runtimePanel, /right:\s*0;/);
  assert.match(runtimePanel, /bottom:\s*0;/);
  assert.match(runtimePanel, /border-radius:\s*20px 0 0 0;/);
  assert.ok(projectPanel, "missing project files panel rule");
  assert.match(projectPanel, /margin:\s*var\(--pf-gutter, 6px\);/);
  assert.match(projectPanel, /border:\s*var\(--pf-surface-border\);/);
  assert.match(projectPanel, /border-radius:\s*var\(--pf-radius-card, 18px\);/);
  assert.match(projectPanel, /background:\s*var\(--pf-surface-background\);/);
  assert.match(projectPanel, /box-shadow:\s*var\(--pf-surface-shadow\);/);
  assert.ok(projectWorkbench, "missing project file workbench rule");
  assert.match(projectWorkbench, /border:\s*var\(--pf-surface-border\);/);
  assert.match(projectWorkbench, /background:\s*var\(--pf-surface-background\);/);
  assert.match(projectWorkbench, /box-shadow:\s*var\(--pf-surface-shadow\);/);
  assert.ok(sidePanel, "missing side chat panel rule");
  assert.match(sidePanel, /margin:\s*0 0 0 8px;/);
  assert.match(sidePanel, /border-radius:\s*20px 0 0 0;/);
});

test("composer floats above a full-height conversation viewport", () => {
  const pane = rule(headerStyles, ".chat-pane");
  const composer = rule(chatStyles, ".composer-shell");
  const messages = rule(coreStyles, ".message-list");

  assert.ok(pane, "missing chat pane rule");
  assert.match(pane, /position:\s*relative;/);
  assert.ok(composer, "missing composer shell rule");
  assert.match(composer, /position:\s*absolute;/);
  assert.match(composer, /bottom:\s*14px;/);
  assert.ok(messages, "missing message list rule");
  assert.match(messages, /--composer-overlay-height/);
  assert.match(messages, /scroll-padding-block-end:/);
});

test("composer overlay clearance follows the live composer height", () => {
  assert.match(chatView, /new ResizeObserver\(syncComposerOverlayHeight\)/);
  assert.match(chatView, /--composer-overlay-height/);
  assert.match(chatView, /ref=\{composerShellRef\}/);
});

test("floating composer uses a legible glass surface", () => {
  const composer = rule(chatStyles, ".composer");
  const darkComposer = rule(chatStyles, 'html[data-theme="dark"] .composer');

  assert.ok(composer, "missing composer rule");
  assert.match(composer, /--composer-surface-base:\s*rgba\([^;]+0\.78\);/);
  assert.match(composer, /background:[\s\S]*var\(--composer-surface-sheen\)/);
  assert.match(composer, /backdrop-filter:\s*blur\(/);
  assert.doesNotMatch(composer, /background:\s*transparent;/);

  assert.ok(darkComposer, "missing dark composer rule");
  assert.match(darkComposer, /--composer-surface-base:\s*rgba\([^;]+0\.76\);/);
  assert.doesNotMatch(darkComposer, /background:\s*transparent;/);
});
