import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const hookSource = await readFile(
  new URL("./useWindowChrome.ts", import.meta.url),
  "utf8",
);
const appSource = await readFile(
  new URL("../../App.tsx", import.meta.url),
  "utf8",
);
const browserDockSource = await readFile(
  new URL("../../components/chat/BrowserDock.tsx", import.meta.url),
  "utf8",
);

test("window dragging uses Tauri native drag regions instead of React IPC handlers", () => {
  assert.doesNotMatch(
    hookSource,
    /startDragging|onTitleMouseDown|onTitleDoubleClick/,
  );
  for (const className of [
    "native-drag-region",
    "sidebar-window-drag-region",
    "content-window-drag-region",
  ]) {
    assert.match(
      appSource,
      new RegExp(
        `className="${className}"[\\s\\S]*?data-tauri-drag-region="true"`,
      ),
    );
  }
  assert.match(
    browserDockSource,
    /className="browser-dock-drag-region"[\s\S]*?data-tauri-drag-region="true"/,
  );
});
