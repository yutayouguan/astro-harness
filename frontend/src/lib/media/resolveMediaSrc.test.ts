import assert from "node:assert/strict";
import test from "node:test";
import {
  absolutizeMediaPath,
  looksLikeLocalPath,
  looksLikeRelativeLocalPath,
  resolveMediaPreviewPath,
  resolveMediaSrc,
  stripFileUrl,
} from "./resolveMediaSrc.ts";

test("looksLikeLocalPath recognizes abs and file urls", () => {
  assert.equal(looksLikeLocalPath("/Users/a/img.png"), true);
  assert.equal(looksLikeLocalPath("C:\\Users\\a\\img.png"), true);
  assert.equal(looksLikeLocalPath("file:///Users/a/img.png"), true);
  assert.equal(looksLikeLocalPath("https://x/y.png"), false);
  assert.equal(looksLikeLocalPath("data:image/png;base64,xx"), false);
  assert.equal(looksLikeLocalPath("blob:http://x/1"), false);
});

test("stripFileUrl decodes file urls", () => {
  assert.equal(stripFileUrl("file:///Users/a/b%20c.png"), "/Users/a/b c.png");
  assert.equal(stripFileUrl("/Users/a/b.png"), "/Users/a/b.png");
});

test("resolveMediaSrc passthrough for http/data/blob", () => {
  assert.equal(resolveMediaSrc("https://ex.com/a.png"), "https://ex.com/a.png");
  assert.equal(
    resolveMediaSrc("data:image/png;base64,abc"),
    "data:image/png;base64,abc",
  );
  assert.equal(resolveMediaSrc("blob:http://localhost/1"), "blob:http://localhost/1");
  assert.equal(resolveMediaSrc(""), null);
  assert.equal(resolveMediaSrc(null), null);
});

test("looksLikeRelativeLocalPath recognizes workspace-relative media", () => {
  assert.equal(
    looksLikeRelativeLocalPath("generated/img-20260715-195717-4d0d50c2.png"),
    true,
  );
  assert.equal(looksLikeRelativeLocalPath("./generated/a.webp"), true);
  assert.equal(looksLikeRelativeLocalPath("/Users/a/img.png"), false);
  assert.equal(looksLikeRelativeLocalPath("https://ex.com/a.png"), false);
  assert.equal(looksLikeRelativeLocalPath(""), false);
});

test("absolutizeMediaPath joins relative under workspace baseDir", () => {
  assert.equal(
    absolutizeMediaPath(
      "generated/img-20260715-195717-4d0d50c2.png",
      "/Users/a/.astro/workspace",
    ),
    "/Users/a/.astro/workspace/generated/img-20260715-195717-4d0d50c2.png",
  );
  assert.equal(
    absolutizeMediaPath(
      "generated/images/img-1.png",
      "/Users/a/.astro/workspace",
    ),
    "/Users/a/.astro/workspace/generated/images/img-1.png",
  );
  assert.equal(
    absolutizeMediaPath("./plans/x.html", "/Users/a/.astro/workspace/"),
    "/Users/a/.astro/workspace/plans/x.html",
  );
  assert.equal(
    absolutizeMediaPath("generated/a.png", null),
    null,
  );
  assert.equal(
    absolutizeMediaPath("/Users/a/.astro/workspace/generated/a.png", "/other"),
    "/Users/a/.astro/workspace/generated/a.png",
  );
  assert.equal(absolutizeMediaPath("https://ex.com/a.png", "/ws"), null);
  assert.equal(
    absolutizeMediaPath("generated/../etc/passwd", "/Users/a/.astro/workspace"),
    null,
  );
});

test("resolveMediaPreviewPath makes generated media absolute for preview and actions", () => {
  assert.equal(
    resolveMediaPreviewPath(
      "generated/images/img-20260716-110609-5fb9089c.jpg",
      "/Users/a/.astro/workspace",
    ),
    "/Users/a/.astro/workspace/generated/images/img-20260716-110609-5fb9089c.jpg",
  );
  assert.equal(
    resolveMediaPreviewPath("https://ex.com/a.png", "/Users/a/.astro/workspace"),
    "https://ex.com/a.png",
  );
});
