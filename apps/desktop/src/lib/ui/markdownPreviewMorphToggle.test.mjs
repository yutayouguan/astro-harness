import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const workspacePanel = await readFile(
  new URL("../../components/workspace/WorkspacePanel.tsx", import.meta.url),
  "utf8",
);

test("workspace markdown preview uses one accessible morphing toggle", () => {
  const toggleComponent = workspacePanel.match(
    /export function MarkdownPreviewToggle\([\s\S]*?\n\}\n\n\/\*\* \u6d4f\u89c8/,
  )?.[0];

  assert.ok(toggleComponent, "missing Markdown preview toggle component");
  assert.equal(
    toggleComponent.match(/<button/g)?.length,
    1,
    "preview and source should share one button",
  );
  assert.match(toggleComponent, /aria-pressed=\{preview\}/);
  assert.match(toggleComponent, /<MorphToggleIcon/);
  assert.match(toggleComponent, /activeIcon=\{EyeData\}/);
  assert.match(toggleComponent, /inactiveIcon=\{FileCode2Data\}/);
  assert.match(toggleComponent, /title=\{label\}/);
  assert.match(
    workspacePanel,
    /onToggle=\{\(\) => setMdModePersist\(docPreview \? "source" : "preview"\)\}/,
  );
});
