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
  assert.doesNotMatch(editor, /fullscreen|exitFullscreen|setFullscreen/);
  assert.match(
    shell,
    /\.page-body--bare:has\(\.loop-editor\)\s*\{[\s\S]*?padding:\s*0;/,
  );
  assert.match(shell, /\.loop-editor\s*\{[\s\S]*?inset:\s*0;/);
  assert.doesNotMatch(shell, /loop-editor--fullscreen/);
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

test("workflow image export captures the complete viewport as PNG", () => {
  assert.match(editor, /import \{ toPng \} from "html-to-image";/);
  assert.match(editor, /reactFlowInstance\.getNodesBounds\(exportNodes\)/);
  assert.match(editor, /await toPng\(viewport,/);
  assert.match(editor, /invoke<string>\("export_loop_png",/);
  assert.doesNotMatch(editor, /svg\.react-flow__edges|XMLSerializer/);
});
