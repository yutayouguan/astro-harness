import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const editorSource = await readFile(
  new URL("../../components/loop/LoopEditor.tsx", import.meta.url),
  "utf8",
);
const canvasStyles = await readFile(
  new URL("../../styles/features/loop/canvas.css", import.meta.url),
  "utf8",
);
const responsiveStyles = await readFile(
  new URL(
    "../../styles/features/loop/responsive-overlays.css",
    import.meta.url,
  ),
  "utf8",
);
const configStyles = await readFile(
  new URL("../../styles/features/loop/config-panel.css", import.meta.url),
  "utf8",
);
const historyStyles = await readFile(
  new URL("../../styles/features/loop/run-history.css", import.meta.url),
  "utf8",
);
const assistantStyles = await readFile(
  new URL("../../styles/features/loop/ai-assistant.css", import.meta.url),
  "utf8",
);

test("workflow edges are calm by default and distinguish branches", () => {
  assert.match(
    editorSource,
    /function edgePresentation[\s\S]*?animated:\s*false/,
  );
  assert.match(editorSource, /loop-edge--branch/);
  assert.match(
    canvasStyles,
    /\.react-flow__edge\.loop-edge--branch[\s\S]*?stroke-dasharray/,
  );
});

test("workflow canvas uses bounded fit and hides the minimap on narrow layouts", () => {
  assert.match(
    editorSource,
    /fitViewOptions=\{\{ padding: 0\.22, maxZoom: 1\.05 \}\}/,
  );
  assert.match(
    responsiveStyles,
    /@media \(max-width: 900px\)[\s\S]*?\.loop-canvas-container \.react-flow__minimap\s*\{[\s\S]*?display:\s*none/,
  );
});

test("workflow inspectors are exclusive and dock beside the canvas", () => {
  assert.match(
    editorSource,
    /const openNodeInspector = useCallback[\s\S]*?setShowHistory\(false\)[\s\S]*?setShowVarsPanel\(false\)[\s\S]*?setShowAiAssistant\(false\)/,
  );
  assert.match(editorSource, /const toggleSidePanel = useCallback/);
  for (const styles of [configStyles, historyStyles, assistantStyles]) {
    assert.match(styles, /position:\s*relative/);
    assert.match(styles, /width:\s*var\(--loop-inspector-width/);
  }
});

test("validation status opens the first invalid node", () => {
  assert.match(
    editorSource,
    /className="loop-statusbar-issues"[\s\S]*?openNodeInspector\(invalidNodes\[0\]\.id\)/,
  );
  assert.match(
    editorSource,
    /if \(invalidNodes\.length > 0\)[\s\S]*?openNodeInspector\(invalidNodes\[0\]\.id\)/,
  );
});
