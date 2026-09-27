import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const read = (relative) => readFile(new URL(relative, import.meta.url), "utf8");

const app = await read("../../App.tsx");
const hook = await read("../../hooks/ui/useInAppBrowserLinks.ts");
const link = await read("../browser/inAppBrowserLink.ts");
const markdown = await read("../../components/chat/ChatMarkdown.tsx");

test("web links are routed to the built-in browser dock", () => {
  assert.match(app, /useInAppBrowserLinks\(\{/);
  assert.match(app, /enabled: nav === "chat" && isTauriRuntime\(\)/);
  assert.match(app, /controlBrowser\("open", \{ url, new_tab: true \}\)/);
  assert.match(app, /setBrowserDockOpen\(true\)/);
  // 评审面板优先级高于浏览器坞，不收起它会导致「点了没反应」
  assert.match(
    app,
    /setReviewState\(null\);\s*\n\s*setBrowserDockOpen\(true\);/,
  );
});

test("the link router only takes over plain left clicks on web links", () => {
  assert.match(hook, /document\.addEventListener\("click", onClick\)/);
  assert.match(hook, /event\.preventDefault\(\)/);
  assert.match(link, /IN_APP_BROWSER_OPT_OUT_ATTR = "data-browser-target"/);
  assert.match(link, /protocol === "http:" \|\| protocol === "https:"/);
  assert.match(
    link,
    /if \(event\.metaKey \|\| event\.ctrlKey \|\| event\.shiftKey \|\| event\.altKey\)/,
  );
  assert.match(link, /if \(\(event\.button \?\? 0\) !== 0\) return null;/);
});

test("chat markdown keeps its webview fallback for unhandled links", () => {
  assert.match(
    markdown,
    /<a href=\{href\} target="_blank" rel="noreferrer noopener">/,
  );
});
