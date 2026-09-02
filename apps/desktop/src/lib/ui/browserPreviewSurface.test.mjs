import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const source = (relative) =>
  fs.readFileSync(path.resolve(here, relative), "utf8");

test("browser preview supports standalone browsing and completed browser tool results", () => {
  const hook = source("../../hooks/chat/useBrowserPreview.ts");
  assert.match(hook, /STORAGE_PREFIX = "astro\.browserPreview\."/);
  assert.match(hook, /name\.startsWith\("browser_"\)/);
  assert.match(hook, /result\?\.astro_browser === true/);
  assert.match(
    hook,
    /storedStatus === "connected" \|\| storedStatus === "connecting"/,
  );
  assert.match(hook, /\? "disconnected"/);
  assert.match(hook, /if \(storedStatus === "closed"\) return null/);
  assert.match(hook, /localStorage\.removeItem/);
  assert.match(hook, /DESKTOP_BROWSER_SESSION_ID = "desktop-browser-default"/);
  assert.match(hook, /sessionId \?\? DESKTOP_BROWSER_SESSION_ID/);
  assert.match(hook, /preview\?\.sessionId === browserSessionId/);

  const dock = source("../../components/chat/BrowserDock.tsx");
  assert.doesNotMatch(dock, /disabled=\{!sessionId/);
  assert.match(dock, /disabled=\{!address\.trim\(\)\}/);
});

test("floating browser preview supports direct manipulation and accessibility fallbacks", () => {
  const component = source("../../components/chat/BrowserPreviewFloat.tsx");
  const css = source("../../styles/features/chat/browser-preview.css");
  assert.match(component, /setPointerCapture/);
  assert.match(component, /onPointerMove/);
  assert.match(
    component,
    /url\.protocol !== "http:" && url\.protocol !== "https:"/,
  );
  assert.match(css, /resize: both/);
  assert.match(css, /@media \(max-width: 620px\)/);
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(css, /@media \(prefers-reduced-transparency: reduce\)/);
});
