import assert from "node:assert/strict";
import test from "node:test";
import {
  WORKSPACE_MD_MODE_KEY,
  isMarkdownFilename,
  readWorkspaceMdMode,
  writeWorkspaceMdMode,
} from "./workspaceMdMode.ts";

test("isMarkdownFilename accepts md and markdown case-insensitively", () => {
  assert.equal(isMarkdownFilename("notes.md"), true);
  assert.equal(isMarkdownFilename("NOTES.MD"), true);
  assert.equal(isMarkdownFilename("doc.markdown"), true);
  assert.equal(isMarkdownFilename("a.ts"), false);
  assert.equal(isMarkdownFilename("readme.mdx"), false);
});

test("readWorkspaceMdMode defaults to source and accepts stored values", () => {
  const g = globalThis as { localStorage?: Storage };
  const store = new Map<string, string>();
  g.localStorage = {
    getItem: (k) => store.get(k) ?? null,
    setItem: (k, v) => {
      store.set(k, v);
    },
    removeItem: (k) => {
      store.delete(k);
    },
    clear: () => store.clear(),
    key: () => null,
    length: 0,
  };
  store.clear();
  assert.equal(readWorkspaceMdMode(), "source");
  store.set(WORKSPACE_MD_MODE_KEY, "preview");
  assert.equal(readWorkspaceMdMode(), "preview");
  store.set(WORKSPACE_MD_MODE_KEY, "source");
  assert.equal(readWorkspaceMdMode(), "source");
  store.set(WORKSPACE_MD_MODE_KEY, "nope");
  assert.equal(readWorkspaceMdMode(), "source");
  writeWorkspaceMdMode("preview");
  assert.equal(store.get(WORKSPACE_MD_MODE_KEY), "preview");
});
