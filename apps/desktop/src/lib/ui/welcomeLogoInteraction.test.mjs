import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const welcome = readFileSync(
  new URL("../../components/chat/ChatWelcome.tsx", import.meta.url),
  "utf8",
);
const chatView = readFileSync(
  new URL("../../components/chat/ChatView.tsx", import.meta.url),
  "utf8",
);
const styles = readFileSync(
  new URL("../../styles/features/chat/markdown.css", import.meta.url),
  "utf8",
);

test("welcome logo is an accessible focus action with click confetti", () => {
  assert.match(welcome, /className="chat-welcome-mark"/);
  assert.match(welcome, /aria-describedby=\{subtitleId\}/);
  assert.match(welcome, /setBurstId/);
  assert.match(
    chatView,
    /onActivate=\{\(\) => textareaRef\.current\?\.focus\(\)\}/,
  );
});

test("welcome logo supports direct drag, spring return, and reduced motion", () => {
  assert.match(welcome, /setPointerCapture/);
  assert.match(welcome, /onLostPointerCapture=\{finishLogoDrag\}/);
  assert.match(welcome, /dragLayerRef\.current\.style\.transform/);
  assert.match(welcome, /dragReturnRef\.current = layer\.animate/);
  assert.match(welcome, /suppressLogoClickRef/);
  assert.match(styles, /\.chat-welcome-mark:active \.chat-welcome-illust/);
  assert.match(
    styles,
    /\.chat-welcome-logo-stack[\s\S]*transform-style: preserve-3d/,
  );
  assert.match(styles, /\.chat-welcome-logo--depth-far/);
  assert.match(styles, /\.chat-welcome-logo--front/);
  assert.match(styles, /@keyframes welcome-logo-confetti/);
  assert.match(styles, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(
    styles,
    /\.chat-welcome-mark-pulse,[\s\S]*\.chat-welcome-burst[\s\S]*display: none/,
  );
});
