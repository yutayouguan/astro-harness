import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { parseClipboardLocalPaths } from "./clipboardLocalPaths.ts";

describe("parseClipboardLocalPaths", () => {
  it("accepts a single absolute path", () => {
    assert.deepEqual(parseClipboardLocalPaths("/Users/me/a.png"), [
      "/Users/me/a.png",
    ]);
  });

  it("accepts multiple path lines", () => {
    assert.deepEqual(parseClipboardLocalPaths("/tmp/a.png\n/tmp/b.jpg"), [
      "/tmp/a.png",
      "/tmp/b.jpg",
    ]);
  });

  it("rejects mixed prose", () => {
    assert.deepEqual(parseClipboardLocalPaths("see /tmp/a.png please"), []);
  });

  it("accepts file:// urls", () => {
    assert.deepEqual(parseClipboardLocalPaths("file:///Users/me/x.webp"), [
      "/Users/me/x.webp",
    ]);
  });
});
