import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  browserLiveWebviewLabel,
  browserWebviewBoundsKey,
  canonicalBrowserUrl,
  createBrowserLiveSurfaceId,
  LIVE_BROWSER_WEBVIEW_PREFIX,
  liveBrowserUserAgentOverride,
  retireBrowserLiveWebview,
  resolveLiveDesiredUrl,
} from "./liveWebview.ts";

const here = path.dirname(fileURLToPath(import.meta.url));

test("live WebView labels are safe and unique across mounts and tabs", () => {
  const firstSurface = createBrowserLiveSurfaceId(1_000);
  const secondSurface = createBrowserLiveSurfaceId(1_000);
  const first = browserLiveWebviewLabel("session/one", "tab:one", firstSurface);
  const second = browserLiveWebviewLabel(
    "session/one",
    "tab:one",
    secondSurface,
  );
  const otherTab = browserLiveWebviewLabel(
    "session/one",
    "tab:two",
    firstSurface,
  );

  assert.match(first, /^[a-zA-Z0-9_:/-]+$/);
  assert.ok(first.startsWith(LIVE_BROWSER_WEBVIEW_PREFIX));
  assert.notEqual(first, second);
  assert.notEqual(first, otherTab);
});

test("live WebView bounds use a stable key for redundant IPC suppression", () => {
  assert.equal(
    browserWebviewBoundsKey({ x: 12, y: 50, width: 680, height: 720 }),
    "12:50:680:720",
  );
});

test("macOS WKWebView receives Safari compatibility tokens", () => {
  const wkWebView =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) " +
    "AppleWebKit/605.1.15 (KHTML, like Gecko)";
  assert.equal(
    liveBrowserUserAgentOverride(wkWebView),
    `${wkWebView} Safari/605.1.15`,
  );

  const safari = `${wkWebView} Version/18.3 Safari/605.1.15`;
  assert.equal(liveBrowserUserAgentOverride(safari), undefined);

  const webView2 =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) " +
    "AppleWebKit/537.36 Chrome/134.0.0.0 Safari/537.36";
  assert.equal(liveBrowserUserAgentOverride(webView2), undefined);
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
  assert.equal(
    canonicalBrowserUrl("https://new.example"),
    "https://new.example/",
  );
});

test("retiring a live WebView hides it before close and tolerates either failure", async () => {
  const calls: string[] = [];
  await retireBrowserLiveWebview({
    async hide() {
      calls.push("hide");
      throw new Error("already hidden");
    },
    async close() {
      calls.push("close");
      throw new Error("already closed");
    },
  });
  assert.deepEqual(calls, ["hide", "close"]);
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
