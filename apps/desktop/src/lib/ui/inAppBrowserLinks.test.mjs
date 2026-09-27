import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const read = (relative) => readFile(new URL(relative, import.meta.url), "utf8");

const app = await read("../../App.tsx");
const hook = await read("../../hooks/ui/useInAppBrowserLinks.ts");
const link = await read("../browser/inAppBrowserLink.ts");
const markdown = await read("../../components/chat/ChatMarkdown.tsx");
const dockStyles = await read("../../styles/features/chat/browser-dock.css");

test("web links are routed to the built-in browser dock", () => {
  assert.match(app, /useInAppBrowserLinks\(\{/);
  assert.match(app, /enabled: isTauriRuntime\(\)/);
  assert.match(app, /controlBrowser\("open", \{ url, new_tab: true \}\)/);
  assert.match(app, /setBrowserDockOpen\(true\)/);
  // 评审面板优先级高于浏览器坞，不收起它会导致「点了没反应」
  assert.match(
    app,
    /setReviewState\(null\);\s*\n\s*setBrowserDockOpen\(true\);/,
  );
});

test("non-chat pages host the dock as an overlay instead of switching pages", () => {
  // 聊天页仍嵌在右侧坞布局里
  assert.match(app, /activeChatRightDock === "browser" \? " has-browser"/);
  // 其他页面用浮层宿主，且不再因为切页而关闭浏览器
  assert.match(app, /browserDockPresence\.mounted && nav !== "chat"/);
  assert.match(app, /className="browser-dock-shell"/);
  assert.doesNotMatch(app, /if \(nav !== "chat"\) setBrowserDockOpen\(false\)/);
});

test("the overlay host only captures the dock itself", () => {
  assert.match(dockStyles, /\.browser-dock-shell\s*\{/);
  assert.match(
    dockStyles,
    /\.browser-dock-shell\s*\{[\s\S]*?pointer-events:\s*none;/,
  );
  assert.match(
    dockStyles,
    /\.browser-dock-shell > \.browser-dock\.is-open\s*\{/,
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
