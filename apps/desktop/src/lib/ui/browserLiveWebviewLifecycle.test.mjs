import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const source = (relative) =>
  fs.readFileSync(path.resolve(here, relative), "utf8");

test("native browser surfaces are hidden before close and stale instances are swept", () => {
  const app = source("../../App.tsx");
  const hook = source("../../hooks/chat/useBrowserLiveWebviews.ts");
  const lifecycle = source("../browser/liveWebview.ts");

  const retireStart = lifecycle.indexOf(
    "export async function retireBrowserLiveWebview",
  );
  const hide = lifecycle.indexOf("await webview.hide()", retireStart);
  const close = lifecycle.indexOf("await webview.close()", retireStart);
  assert.ok(retireStart >= 0 && hide > retireStart && close > hide);

  assert.match(hook, /Webview\.getAll\(\)/);
  assert.match(hook, /startsWith\(LIVE_BROWSER_WEBVIEW_PREFIX\)/);
  assert.match(hook, /webview\.window\.label === currentWindowLabel/);
  assert.match(hook, /\.map\(retireBrowserLiveWebview\)/);
  assert.match(hook, /await cleanupStaleBrowserLiveWebviews\(\)/);
  assert.match(hook, /void retireBrowserLiveWebview\(entry\.webview\)/);

  assert.match(app, /void cleanupStaleBrowserLiveWebviews\(\);/);
  assert.match(app, /if \(nav !== "chat"\) setBrowserDockOpen\(false\);/);
});
