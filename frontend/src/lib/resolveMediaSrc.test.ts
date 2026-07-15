import assert from "node:assert/strict";
import test from "node:test";
import {
  looksLikeLocalPath,
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
