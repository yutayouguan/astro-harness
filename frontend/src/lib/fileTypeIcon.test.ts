import assert from "node:assert/strict";
import test from "node:test";
import {
  fileExt,
  isExternalOnlyFile,
  mediaKindOf,
  resolveFileType,
} from "./fileTypeIcon.ts";

test("fileExt strips leading dots of name but keeps extension", () => {
  assert.equal(fileExt("SOUL.md"), "md");
  assert.equal(fileExt("archive.tar.gz"), "gz");
  assert.equal(fileExt(".gitignore"), "");
  assert.equal(fileExt("Makefile"), "");
});

test("office and archives resolve with dedicated icons and external open", () => {
  assert.equal(resolveFileType("report.docx").kind, "word");
  assert.equal(resolveFileType("book.xlsx").kind, "sheet");
  assert.equal(resolveFileType("deck.pptx").kind, "slides");
  assert.equal(resolveFileType("spec.pdf").kind, "pdf");
  assert.equal(resolveFileType("src.zip").kind, "archive");
  assert.ok(isExternalOnlyFile("deck.pptx"));
  assert.ok(isExternalOnlyFile("src.zip"));
});

test("language extensions map to distinct glyph kinds", () => {
  assert.equal(resolveFileType("app.ts").kind, "code-ts");
  assert.equal(resolveFileType("app.tsx").kind, "code-ts");
  assert.equal(resolveFileType("main.rs").kind, "code-rs");
  assert.equal(resolveFileType("main.py").kind, "code-py");
  assert.equal(resolveFileType("Main.java").kind, "code-java");
  assert.equal(resolveFileType("index.html").kind, "code-web");
  assert.equal(resolveFileType("run.sh").kind, "code-shell");
  assert.equal(resolveFileType("query.sql").kind, "code-sql");
});

test("media and folders", () => {
  assert.equal(mediaKindOf("a.png"), "image");
  assert.equal(mediaKindOf("a.mp4"), "video");
  assert.equal(resolveFileType("docs", true).kind, "folder");
  assert.equal(resolveFileType("notes.csv").kind, "sheet");
  assert.equal(resolveFileType("notes.csv").open, "text");
});
