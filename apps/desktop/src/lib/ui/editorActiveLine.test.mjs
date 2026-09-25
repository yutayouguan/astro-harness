import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const editorUrl = new URL(
  "../../components/workspace/WorkspaceEditor.tsx",
  import.meta.url,
);
const themeUrl = new URL("../filespace/codeMirrorLanguage.ts", import.meta.url);
const workspaceCssUrl = new URL(
  "../../styles/features/workspace.css",
  import.meta.url,
);

test("clicking a line leaves no whole-line highlight in the file editor", async () => {
  const [editor, theme, css] = await Promise.all([
    readFile(editorUrl, "utf8"),
    readFile(themeUrl, "utf8"),
    readFile(workspaceCssUrl, "utf8"),
  ]);

  assert.match(editor, /highlightActiveLine:\s*false/);
  assert.match(editor, /highlightActiveLineGutter:\s*false/);
  assert.doesNotMatch(editor, /highlightActiveLine:\s*true/);
  assert.doesNotMatch(editor, /highlightActiveLineGutter:\s*true/);
  assert.doesNotMatch(theme, /lineHighlight:/);
  assert.doesNotMatch(theme, /gutterActiveForeground:/);
  assert.doesNotMatch(css, /cm-activeLine/);
});

test("the editor still marks real selections and the caret", async () => {
  const [theme, css] = await Promise.all([
    readFile(themeUrl, "utf8"),
    readFile(workspaceCssUrl, "utf8"),
  ]);

  assert.match(theme, /selection:\s*"rgba\(139, 92, 246, 0\.18\)"/);
  assert.match(theme, /selectionMatch:\s*"rgba\(139, 92, 246, 0\.1\)"/);
  assert.match(theme, /caret:\s*"#7c3aed"/);
  assert.match(css, /\.ws-codemirror \.cm-selectionBackground\s*\{/);
  assert.match(css, /\.ws-codemirror \.cm-cursor\s*\{/);
});
