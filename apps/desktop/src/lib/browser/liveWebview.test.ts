import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  browserLiveWebviewLabel,
  canonicalBrowserUrl,
  createBrowserLiveSurfaceId,
  LIVE_BROWSER_WEBVIEW_PREFIX,
  resolveLiveDesiredUrl,
} from "./liveWebview.ts";

const here = path.dirname(fileURLToPath(import.meta.url));

test("live WebView labels are safe and unique across mounts and tabs", () => {
  const firstSurface = createBrowserLiveSurfaceId(1_000);
  const secondSurface = createBrowserLiveSurfaceId(1_000);
  const first = browserLiveWebviewLabel("session/one", "tab:one", firstSurface);
  const second = browserLiveWebviewLabel("session/one", "tab:one", secondSurface);
  const otherTab = browserLiveWebviewLabel("session/one", "tab:two", firstSurface);

  assert.match(first, /^[a-zA-Z0-9_:/-]+$/);
  assert.ok(first.startsWith(LIVE_BROWSER_WEBVIEW_PREFIX));
  assert.notEqual(first, second);
  assert.notEqual(first, otherTab);
});

test("pending native navigation wins until the automation session catches up", () => {
  assert.equal(
    resolveLiveDesiredUrl("https://old.example/", "https://new.example/"),
    "https://new.example/",
  );
  assert.equal(
    resolveLiveDesiredUrl("https://new.example", "https://new.example/"),
    "https://new.example",
  );
  assert.equal(canonicalBrowserUrl("https://new.example"), "https://new.example/");
});

test("browser content fallback inherits the same glass material as its toolbar", () => {
  const css = fs.readFileSync(
    path.resolve(here, "../../styles/features/chat/browser-dock.css"),
    "utf8",
  );
  const viewport = css.slice(
    css.indexOf(".browser-viewport {"),
    css.indexOf(".browser-viewport > img"),
  );
  const placeholder = css.slice(
    css.indexOf(".browser-live-placeholder {"),
    css.indexOf(".browser-empty,"),
  );

  assert.match(viewport, /background:\s*transparent/);
  assert.match(placeholder, /background:\s*transparent/);
});
