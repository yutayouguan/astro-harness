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
const loopCommands = await readFile(
  new URL("../../../src-tauri/src/commands/automation/loops.rs", import.meta.url),
  "utf8",
);
const tauri = await readFile(
  new URL("../../../src-tauri/src/lib.rs", import.meta.url),
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
    /\.loop-editor-body\s*\{[\s\S]*?background:\s*color-mix\(in srgb, var\(--glass-panel\) 18%, transparent\);/,
  );
  const bodyRule = shell.match(/\.loop-editor-body\s*\{(?<body>[\s\S]*?)\n\}/)
    ?.groups?.body;
  assert.ok(bodyRule);
  assert.doesNotMatch(bodyRule, /backdrop-filter/);
  assert.match(
    shell,
    /\.loop-editor-toolbar\s*\{[\s\S]*?background:\s*var\(--glass-fill-soft\);[\s\S]*?backdrop-filter:\s*var\(--backdrop-glass\);/,
  );
  assert.match(
    canvas,
    /\.loop-canvas-container \.react-flow__background\s*\{[\s\S]*?background:\s*transparent;/,
  );
});

test("workflow export writes SVG and offers a system-app open action", () => {
  assert.match(editor, /import \{ buildWorkflowSvg \}/);
  assert.match(editor, /const svg = buildWorkflowSvg\(/);
  assert.match(editor, /invoke<string>\("export_loop_svg",/);
  assert.match(editor, /invoke\("open_loop_export",/);
  assert.match(editor, /actionLabel: t\("workspace\.menu\.open"\)/);
  assert.doesNotMatch(editor, /toPng|export_loop_png|data:image\/png/);
  assert.match(loopCommands, /pub async fn open_loop_export/);
  assert.doesNotMatch(loopCommands, /export_loop_png|decode_png_data_url/);
  assert.match(tauri, /commands::loops::open_loop_export/);
  assert.doesNotMatch(tauri, /commands::loops::export_loop_png/);
});
