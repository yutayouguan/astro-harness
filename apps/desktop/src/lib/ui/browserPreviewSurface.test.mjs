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
  assert.match(hook, /isNavigationAction\(action\)/);
  assert.match(hook, /isNavigationAction\(name\)/);
  const navigationActions = hook.slice(
    hook.indexOf("const NAVIGATION_ACTIONS"),
    hook.indexOf("function isNavigationAction"),
  );
  assert.match(navigationActions, /"open"/);
  assert.match(navigationActions, /"browser_open"/);
  assert.doesNotMatch(
    navigationActions,
    /click|scroll|key|resize|snapshot|screenshot/,
  );

  const dock = source("../../components/chat/BrowserDock.tsx");
  assert.doesNotMatch(dock, /disabled=\{!sessionId/);
  assert.match(dock, /disabled=\{!address\.trim\(\)\}/);
  assert.match(dock, /open \? " is-open" : ""/);
  assert.match(dock, /active: open/);
  assert.doesNotMatch(dock, /browser-dock-resizer/);
  assert.doesNotMatch(dock, /browser-dock-expand/);
  assert.doesNotMatch(dock, /BROWSER_DOCK_WIDTH_KEY/);
  assert.match(dock, /useBrowserLiveWebviews/);
  assert.match(dock, /browser-live-placeholder/);
  assert.match(dock, /liveWebview\.isLoading/);

  const headerStart = dock.indexOf('<header className="browser-dock-header">');
  const tabsStart = dock.indexOf('className="browser-tabs"', headerStart);
  const headerEnd = dock.indexOf("</header>", headerStart);
  assert.doesNotMatch(dock, /browser-dock-title/);
  assert.ok(
    headerStart >= 0 && tabsStart > headerStart && tabsStart < headerEnd,
  );
  assert.match(dock, /browser-dock-overflow-trigger/);
  assert.match(dock, /role="menu"/);
  assert.match(dock, /MoreVertical/);
});

test("browser dock uses a native child WebView with a screenshot fallback", () => {
  const hook = source("../../hooks/chat/useBrowserLiveWebviews.ts");
  const commands = source("../../../src-tauri/src/commands/browser.rs");
  const capabilities = source("../../../src-tauri/capabilities/default.json");

  assert.match(hook, /new Webview\(getCurrentWindow\(\), label/);
  assert.match(hook, /setPosition\(new LogicalPosition/);
  assert.match(hook, /setSize\(new LogicalSize/);
  assert.match(hook, /browser-live-page-load/);
  assert.match(hook, /browser_live_webview_control/);
  assert.match(commands, /fn browser_live_webview_control/);
  assert.match(commands, /fn live_browser_plugin/);
  assert.match(commands, /validate_live_webview_url/);
  for (const permission of [
    "core:webview:allow-create-webview",
    "core:webview:allow-set-webview-position",
    "core:webview:allow-set-webview-size",
    "core:webview:allow-webview-show",
    "core:webview:allow-webview-hide",
    "core:webview:allow-webview-close",
  ]) {
    assert.match(capabilities, new RegExp(permission));
  }
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
