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
  assert.match(canvas, /await import\(\s*"vgpu"\s*\)/);
  assert.match(canvas, /await ledEffect\.compile\(canvasSurface\)/);
  assert.match(canvas, /dpr: \[1, 2\]/);
  assert.match(canvas, /canvasSurface\.clearColor = \[0, 0, 0, 0\]/);
});

test("3D logo lighting follows pointer and respects reduced motion and fallback", () => {
  assert.match(canvas, /addEventListener\("pointermove"/);
  assert.match(canvas, /prefers-reduced-motion: reduce/);
  assert.match(
    canvas,
    /frame\(gpu, \(currentFrame\) => draw\(currentFrame, 0\)\)/,
  );
  assert.match(canvas, /canvas\.dataset\.ready = "false"/);
  assert.match(canvas, /ASTRO_MARK_PATH/);
  assert.match(canvas, /className="chat-welcome-logo-stack"/);
  assert.match(canvas, /chat-welcome-logo--depth-far/);
  assert.match(canvas, /className="chat-welcome-logo-lighting-fallback"/);
  assert.match(
    styles,
    /\.chat-welcome-logo-lighting\[data-ready="true"\]\s*\+\s*\.chat-welcome-logo-lighting-fallback/,
  );
  assert.match(styles, /@keyframes welcome-logo-material-sweep/);
  assert.doesNotMatch(canvas, /inside_triangle|chat-welcome-led-fallback/);
  assert.match(styles, /\.chat-welcome-orb/);
});

test("welcome logo material and gradient copy inherit the active theme tone", () => {
  assert.match(canvas, /tone: vec4f/);
  assert.match(canvas, /accent: vec4f/);
  assert.match(canvas, /readThemeColors\(themeRoot\)/);
  assert.match(
    canvas,
    /attributeFilter: \[[\s\S]*?"data-theme"[\s\S]*?"data-tone"[\s\S]*?"data-color-style"[\s\S]*?"style"/,
  );
  assert.doesNotMatch(canvas, /let (?:blue|violet|cyan) = vec3f/);

  assert.match(
    styles,
    /\.chat-welcome-logo-stack \{[\s\S]*?--astro-mark-c1: var\(--tone/,
  );
  assert.match(
    styles,
    /\.chat-welcome-wordmark \{[\s\S]*?var\(--accent-2, var\(--tone/,
  );
  assert.match(
    styles,
    /\.chat-welcome-title-brand \{[\s\S]*?var\(--tone\)[\s\S]*?var\(--accent-2, var\(--tone\)/,
  );
});
