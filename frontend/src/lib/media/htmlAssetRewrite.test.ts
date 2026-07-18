import assert from "node:assert/strict";
import test from "node:test";
import {
  isAbsoluteDir,
  resolveAgainstDir,
  rewriteHtmlRelativeAssets,
} from "./htmlAssetRewrite.ts";

// 模拟 Tauri macOS 版 convertFileSrc：整路径 encodeURIComponent
const fakeConvert = (p: string) =>
  `asset://localhost/${encodeURIComponent(p)}`;

test("resolveAgainstDir handles same-dir, parent and decodes CJK", () => {
  const dir = "/Users/a/.astro/workspace/generated/html";
  assert.equal(
    resolveAgainstDir(dir, "images/x.jpg"),
    "/Users/a/.astro/workspace/generated/html/images/x.jpg",
  );
  assert.equal(
    resolveAgainstDir(dir, "../images/x.jpg"),
    "/Users/a/.astro/workspace/generated/images/x.jpg",
  );
  assert.equal(
    resolveAgainstDir(dir, "./a/../b.png"),
    "/Users/a/.astro/workspace/generated/html/b.png",
  );
  assert.equal(
    resolveAgainstDir(dir, "../images/%E6%9D%BE%E8%8C%B8.jpg"),
    "/Users/a/.astro/workspace/generated/images/松茸.jpg",
  );
});

test("rewrite converts ../images img/url() to absolute asset URL", () => {
  const dir = "/Users/a/.astro/workspace/generated/html";
  const html =
    `<img src="../images/松茸.jpg"/>` +
    `<div style="background:url('../images/云南.jpg')"></div>`;
  const out = rewriteHtmlRelativeAssets(html, dir, fakeConvert);
  assert.ok(
    out.includes(
      fakeConvert("/Users/a/.astro/workspace/generated/images/松茸.jpg"),
    ),
  );
  assert.ok(
    out.includes(
      fakeConvert("/Users/a/.astro/workspace/generated/images/云南.jpg"),
    ),
  );
  // 改写后不应再有相对 ../images 引用
  assert.ok(!out.includes("../images/"));
});

test("rewrite leaves absolute / protocol / anchor refs untouched", () => {
  const dir = "/Users/a/ws";
  const html =
    `<img src="https://x/a.png">` +
    `<img src="data:image/png;base64,AAAA">` +
    `<img src="/abs/a.png">` +
    `<a href="#top">t</a>` +
    `<img src="asset://localhost/x">`;
  assert.equal(rewriteHtmlRelativeAssets(html, dir, fakeConvert), html);
});

test("rewrite is a no-op when dir is not absolute", () => {
  const html = `<img src="images/x.jpg">`;
  assert.equal(
    rewriteHtmlRelativeAssets(html, "generated/html", fakeConvert),
    html,
  );
});

test("isAbsoluteDir distinguishes absolute vs relative", () => {
  assert.equal(isAbsoluteDir("/Users/a"), true);
  assert.equal(isAbsoluteDir("C:\\Users\\a"), true);
  assert.equal(isAbsoluteDir("generated/html"), false);
});
