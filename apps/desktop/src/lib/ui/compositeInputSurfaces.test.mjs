import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const read = (path) => readFile(new URL(path, import.meta.url), "utf8");

/** 有外壳、内部输入框却没有任何自身方框的组合字段。 */
const SHELLS = [
  {
    shell: "project-edit-name-row",
    markup: "../../components/chat/ProjectEditDialog.tsx",
    input: ".project-edit-name-input",
    css: "../../styles/features/shell/layout/project-dialog.css",
  },
  {
    shell: "project-icon-picker-search",
    markup: "../../components/chat/ProjectFolderIconPicker.tsx",
    input: ".project-icon-picker-search input",
    css: "../../components/chat/ProjectFolderIcon.css",
  },
  {
    shell: "loop-canvas-search",
    markup: "../../components/loop/LoopEditor.tsx",
    input: ".loop-canvas-search input",
    css: "../../styles/features/loop/responsive-overlays.css",
  },
  {
    shell: "loop-node-picker-search",
    markup: "../../components/loop/LoopEditor.tsx",
    input: ".loop-node-picker-search-input",
    css: "../../styles/features/loop/editor-shell.css",
  },
  {
    shell: "a2ui-clarify-custom",
    markup: "../../a2ui/ClarifyWizard.tsx",
    input: ".a2ui-clarify-custom-input",
    css: "../../styles/features/chat/activity.css",
  },
];

const files = new Map();
async function readOnce(path) {
  if (!files.has(path)) files.set(path, await read(path));
  return files.get(path);
}

/** 取出单个规则块（不处理嵌套 at-rule）。 */
function ruleBody(css, selector) {
  const match = css.match(
    new RegExp(
      `${selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\s*\\{([^}]*)\\}`,
    ),
  );
  return match ? match[1] : null;
}

test("composite shells own the focus edge instead of their bare input", async () => {
  const runtime = await readOnce("./inputFocus.ts");
  const material = await readOnce("../../styles/materials/soft-focus.css");

  // 运行时契约仍然只认显式声明的外壳。
  assert.match(runtime, /"\[data-input-surface\]"/);
  assert.match(material, /\[data-input-focus\]\s*\{[\s\S]*?outline:/);
  assert.match(material, /border-color: transparent/);

  for (const { shell, markup, input, css } of SHELLS) {
    const source = await readOnce(markup);
    const marked = new RegExp(
      `className=(?:"${shell}"|\\{\`${shell}[^\`]*\`\\})[^>]*data-input-surface`,
    );
    assert.match(
      source,
      marked,
      `${shell} must carry data-input-surface so the ring follows the shell`,
    );

    // 内部输入框没有自己的边框/背景，所以它绝不能自己承载焦点环，
    // 否则就是用户截图里那圈方框。
    const body = ruleBody(await readOnce(css), input);
    assert.ok(body, `${input} rule missing in ${css}`);
    assert.match(
      body,
      /border:\s*(?:none|0)\s*;/,
      `${input} must not paint its own border`,
    );
    assert.match(
      body,
      /background:\s*(?:transparent|none)\s*;/,
      `${input} must not paint its own background`,
    );
  }
});
