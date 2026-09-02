import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const source = (relative) =>
  fs.readFileSync(path.resolve(here, relative), "utf8");

test("browser tabs render website favicons with a globe fallback", () => {
  const component = source("../../components/chat/BrowserDock.tsx");
  const hook = source("../../hooks/chat/useBrowserPreview.ts");
  const css = source("../../styles/features/chat/browser-dock.css");

  assert.match(hook, /row\.favicon_url \?\? row\.faviconUrl/);
  assert.match(component, /<BrowserTabIcon tab=\{tab\} \/>/);
  assert.match(component, /onError=\{\(\) => setFailedUrl\(tab\.faviconUrl\)\}/);
  assert.match(component, /browser-tab-favicon-fallback/);
  assert.match(css, /\.browser-tab-favicon/);
});
