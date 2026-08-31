import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const canvas = readFileSync(
  new URL("../../components/chat/WelcomeLedCanvas.tsx", import.meta.url),
  "utf8",
);
const welcome = readFileSync(
  new URL("../../components/chat/ChatWelcome.tsx", import.meta.url),
  "utf8",
);
const styles = readFileSync(
  new URL("../../styles/features/chat/markdown.css", import.meta.url),
  "utf8",
);

test("welcome page mounts a lazily loaded vgpu LED backdrop", () => {
  assert.match(welcome, /<WelcomeLedCanvas \/>/);
  assert.match(canvas, /await import\("vgpu"\)/);
  assert.match(canvas, /await ledEffect\.compile\(canvasSurface\)/);
  assert.match(canvas, /dpr: \[1, 2\]/);
  assert.match(canvas, /canvasSurface\.clearColor = \[0, 0, 0, 0\]/);
});

test("LED backdrop follows pointer and respects reduced motion and fallback", () => {
  assert.match(canvas, /addEventListener\("pointermove"/);
  assert.match(canvas, /prefers-reduced-motion: reduce/);
  assert.match(canvas, /frame\(gpu, \(currentFrame\) => draw\(currentFrame, 0\)\)/);
  assert.match(canvas, /canvas\.dataset\.ready = "false"/);
  assert.match(canvas, /className="chat-welcome-led-fallback"/);
  assert.match(styles, /\.chat-welcome-led-canvas\[data-ready="true"\] \+ \.chat-welcome-led-fallback/);
  assert.match(styles, /@keyframes welcome-led-dots-travel/);
  assert.match(styles, /\.chat-welcome-orb/);
});
