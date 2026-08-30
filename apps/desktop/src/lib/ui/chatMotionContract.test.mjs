import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const root = new URL("../../", import.meta.url);

async function source(path) {
  return readFile(new URL(path, root), "utf8");
}

test("high-frequency chat surfaces use restrained, non-bouncy motion", async () => {
  const [core, rightPanelCss, sideChatCss, toastCss] = await Promise.all([
    source("styles/features/chat/core.css"),
    source("styles/features/chat/right-panel.css"),
    source("styles/features/chat/side-chat.css"),
    source("styles/components/toast.css"),
  ]);

  assert.match(core, /animation: rise 0\.16s var\(--ease-out/);
  assert.doesNotMatch(rightPanelCss, /chat-right-slide-in|chat-right-slide-out/);
  assert.doesNotMatch(sideChatCss, /chat-right-slide-in/);
  assert.doesNotMatch(toastCss, /astro-toast-in|astro-toast-icon-bounce/);
});

test("dismissible chat surfaces retain an exit presence lifecycle", async () => {
  const [app, rightPanel, sideChat, toast, plusMenu, preview] = await Promise.all([
    source("App.tsx"),
    source("components/chat/ChatRightPanel.tsx"),
    source("components/chat/SideChatPanel.tsx"),
    source("components/ui/Toast.tsx"),
    source("components/chat/ComposerPlusMenu.tsx"),
    source("components/chat/ComposerContextPreview.tsx"),
  ]);

  assert.match(app, /<AnimatePresence initial=\{false\}>/);
  assert.match(rightPanel, /<motion\.aside/);
  assert.match(sideChat, /<motion\.aside/);
  assert.match(toast, /<AnimatePresence initial=\{false\}>[\s\S]*?exit=/);
  assert.match(plusMenu, /<AnimatePresence initial=\{false\}>[\s\S]*?transformOrigin: "left bottom"/);
  assert.match(preview, /<AnimatePresence initial=\{false\}>[\s\S]*?<motion\.section/);
});

test("new motion keeps a reduced-motion opacity path", async () => {
  const files = await Promise.all([
    source("components/chat/ChatRightPanel.tsx"),
    source("components/chat/SideChatPanel.tsx"),
    source("components/ui/Toast.tsx"),
    source("components/chat/ComposerPlusMenu.tsx"),
    source("components/chat/ComposerContextPreview.tsx"),
  ]);

  for (const content of files) {
    assert.match(content, /useReducedMotion\(\)/);
    assert.match(content, /reducedMotion \? \{ opacity: 0 \}/);
  }
});

test("side chat surface matches the project-files enter geometry and timing", async () => {
  const [sideChat, sideChatCss] = await Promise.all([
    source("components/chat/SideChatPanel.tsx"),
    source("styles/features/chat/side-chat.css"),
  ]);

  assert.match(sideChat, /opacity: 0, x: 18, scale: 0\.985/);
  assert.match(sideChat, /x:\s*\{ duration: 0\.3, ease: DOCK_EASE \}/);
  assert.match(sideChat, /scale:\s*\{ duration: 0\.3, ease: DOCK_EASE \}/);
  assert.match(sideChat, /opacity:\s*\{ duration: 0\.18, ease: "easeOut" \}/);
  assert.match(
    sideChatCss,
    /@media \(prefers-reduced-motion: reduce\)[\s\S]*?\.side-chat-dock[\s\S]*?transition:\s*none;/,
  );
});

test("menus and dialogs avoid spring overshoot in routine interactions", async () => {
  const [selectMenu, agentPicker, dialog] = await Promise.all([
    source("styles/components/select-menu.css"),
    source("styles/components/agent-picker.css"),
    source("styles/components/dialog.css"),
  ]);

  assert.match(selectMenu, /select-menu-in 0\.15s var\(--ease-out/);
  assert.doesNotMatch(selectMenu, /scale\(1\.01\)/);
  assert.match(agentPicker, /agent-picker-in 0\.15s var\(--ease-out/);
  assert.doesNotMatch(agentPicker, /scale\(1\.01\)/);
  assert.match(dialog, /app-dialog-rise-in 0\.22s var\(--ease-out/);
  assert.match(dialog, /app-dialog-fade-in 0\.1s ease-out/);
});
