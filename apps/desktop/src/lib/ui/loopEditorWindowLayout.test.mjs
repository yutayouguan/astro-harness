import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const editor = await readFile(
  new URL("../../components/loop/LoopEditor.tsx", import.meta.url),
  "utf8",
);
const shell = await readFile(
  new URL("../../styles/features/loop/editor-shell.css", import.meta.url),
  "utf8",
);
const canvas = await readFile(
  new URL("../../styles/features/loop/canvas.css", import.meta.url),
  "utf8",
);

test("workflow editor opens as a full-window workspace", () => {
  assert.match(
    editor,
    /const \[fullscreen, setFullscreen\] = useState\(true\);/,
  );
  assert.match(
    shell,
    /\.page-body--bare:has\(\.loop-editor--fullscreen\)\s*\{[\s\S]*?padding:\s*0;/,
  );
  assert.match(shell, /\.loop-editor--fullscreen\s*\{[\s\S]*?inset:\s*0;/);
  assert.doesNotMatch(
    shell,
    /\.loop-editor--fullscreen \.loop-editor-toolbar\s*\{/,
  );
});

test("workflow content keeps the wallpaper visible through frosted glass", () => {
  assert.match(
    shell,
    /\.loop-editor-body\s*\{[\s\S]*?background:\s*var\(--glass-fill-soft\);[\s\S]*?backdrop-filter:\s*var\(--backdrop-glass\);/,
  );
  assert.match(
    shell,
    /\.loop-editor-toolbar\s*\{[\s\S]*?background:\s*var\(--glass-fill-soft\);[\s\S]*?backdrop-filter:\s*var\(--backdrop-glass\);/,
  );
  assert.match(
    canvas,
    /\.loop-canvas-container \.react-flow__background\s*\{[\s\S]*?background:\s*transparent;/,
  );
});
