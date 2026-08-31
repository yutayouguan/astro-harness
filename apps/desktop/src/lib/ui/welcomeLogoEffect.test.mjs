import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const canvas = readFileSync(
  new URL("../../components/chat/WelcomeLogoEffect.tsx", import.meta.url),
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

test("welcome logo mounts a lazily loaded vgpu material layer", () => {
  assert.match(welcome, /<WelcomeLogoEffect \/>/);
  assert.match(canvas, /await import\("vgpu"\)/);
  assert.match(canvas, /await ledEffect\.compile\(canvasSurface\)/);
  assert.match(canvas, /dpr: \[1, 2\]/);
  assert.match(canvas, /canvasSurface\.clearColor = \[0, 0, 0, 0\]/);
});

test("3D logo lighting follows pointer and respects reduced motion and fallback", () => {
  assert.match(canvas, /addEventListener\("pointermove"/);
  assert.match(canvas, /prefers-reduced-motion: reduce/);
  assert.match(canvas, /frame\(gpu, \(currentFrame\) => draw\(currentFrame, 0\)\)/);
  assert.match(canvas, /canvas\.dataset\.ready = "false"/);
  assert.match(canvas, /ASTRO_MARK_PATH/);
  assert.match(canvas, /className="chat-welcome-logo-stack"/);
  assert.match(canvas, /chat-welcome-logo--depth-far/);
  assert.match(canvas, /className="chat-welcome-logo-lighting-fallback"/);
  assert.match(styles, /\.chat-welcome-logo-lighting\[data-ready="true"\] \+ \.chat-welcome-logo-lighting-fallback/);
  assert.match(styles, /@keyframes welcome-logo-material-sweep/);
  assert.doesNotMatch(canvas, /inside_triangle|chat-welcome-led-fallback/);
  assert.match(styles, /\.chat-welcome-orb/);
});
