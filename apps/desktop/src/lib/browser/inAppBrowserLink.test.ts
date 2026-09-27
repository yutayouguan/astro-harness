import assert from "node:assert/strict";
import test from "node:test";
import {
  IN_APP_BROWSER_OPT_OUT_ATTR,
  isWebLinkHref,
  resolveInAppBrowserLink,
} from "./inAppBrowserLink.ts";

/** 最小 DOM 替身：只实现 `closest("a[href]")` 用到的遍历。 */
function fakeTarget(anchor: {
  href: string | null;
  browserTarget?: string;
  wrapDepth?: number;
}) {
  const element = {
    tagName: "A",
    closest: (selector: string) => (selector === "a[href]" ? element : null),
    getAttribute(name: string) {
      if (name === "href") return anchor.href;
      if (name === IN_APP_BROWSER_OPT_OUT_ATTR)
        return anchor.browserTarget ?? null;
      return null;
    },
  };
  let node: unknown = element;
  for (let i = 0; i < (anchor.wrapDepth ?? 0); i += 1) {
    node = { closest: () => element };
  }
  return node;
}

const available = { dockAvailable: true };

test("only absolute http(s) links count as web links", () => {
  assert.equal(isWebLinkHref("https://example.com/a?b=1"), true);
  assert.equal(isWebLinkHref("http://127.0.0.1:1420/"), true);
  assert.equal(isWebLinkHref("  https://example.com  "), true);
  assert.equal(isWebLinkHref("mailto:a@b.com"), false);
  assert.equal(isWebLinkHref("file:///tmp/a.html"), false);
  assert.equal(isWebLinkHref("/tmp/a.html"), false);
  assert.equal(isWebLinkHref("#anchor"), false);
  assert.equal(isWebLinkHref(undefined), false);
});

test("resolves plain left clicks on web links", () => {
  assert.equal(
    resolveInAppBrowserLink(
      { target: fakeTarget({ href: "https://example.com/news" }), button: 0 },
      available,
    ),
    "https://example.com/news",
  );
  // 点击锚点内部元素（如链接文字）同样命中
  assert.equal(
    resolveInAppBrowserLink(
      { target: fakeTarget({ href: "https://example.com/x", wrapDepth: 2 }) },
      available,
    ),
    "https://example.com/x",
  );
});

test("leaves modifier clicks and non-web targets to the webview", () => {
  const link = () => fakeTarget({ href: "https://example.com/x" });
  assert.equal(
    resolveInAppBrowserLink({ target: link(), metaKey: true }, available),
    null,
  );
  assert.equal(
    resolveInAppBrowserLink({ target: link(), ctrlKey: true }, available),
    null,
  );
  assert.equal(
    resolveInAppBrowserLink({ target: link(), shiftKey: true }, available),
    null,
  );
  assert.equal(
    resolveInAppBrowserLink({ target: link(), altKey: true }, available),
    null,
  );
  assert.equal(
    resolveInAppBrowserLink({ target: link(), button: 1 }, available),
    null,
  );
  assert.equal(
    resolveInAppBrowserLink(
      { target: link(), defaultPrevented: true },
      available,
    ),
    null,
  );
  assert.equal(
    resolveInAppBrowserLink(
      { target: fakeTarget({ href: "mailto:a@b.com" }) },
      available,
    ),
    null,
  );
  assert.equal(
    resolveInAppBrowserLink({ target: { closest: () => null } }, available),
    null,
  );
  assert.equal(resolveInAppBrowserLink({ target: null }, available), null);
});

test("honours the explicit system-browser opt-out", () => {
  assert.equal(
    resolveInAppBrowserLink(
      {
        target: fakeTarget({
          href: "https://example.com/x",
          browserTarget: "external",
        }),
      },
      available,
    ),
    null,
  );
});

test("stays out of the way when the in-app browser is unavailable", () => {
  assert.equal(
    resolveInAppBrowserLink(
      { target: fakeTarget({ href: "https://example.com/x" }) },
      { dockAvailable: false },
    ),
    null,
  );
});
