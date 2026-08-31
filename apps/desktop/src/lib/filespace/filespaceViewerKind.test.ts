import assert from "node:assert/strict";
import test from "node:test";
import { filespaceViewerKind } from "./filespaceViewerKind.ts";

test("filespaceViewerKind maps common names", () => {
  assert.equal(filespaceViewerKind({ name: "a.md" }), "markdown");
  assert.equal(filespaceViewerKind({ name: "README.markdown" }), "markdown");
  assert.equal(filespaceViewerKind({ name: "index.html" }), "html");
  assert.equal(filespaceViewerKind({ name: "main.rs" }), "text");
  assert.equal(filespaceViewerKind({ name: "a.png" }), "image");
  assert.equal(filespaceViewerKind({ name: "a.mp4" }), "video");
  assert.equal(filespaceViewerKind({ name: "a.mp3" }), "audio");
  assert.equal(filespaceViewerKind({ name: "spec.pdf" }), "pdf");
  assert.equal(filespaceViewerKind({ name: "deck.pptx" }), "external");
});

test("filespaceViewerKind respects missing and mime/category fallbacks", () => {
  assert.equal(
    filespaceViewerKind({ name: "gone.bin", missing: true }),
    "missing",
  );
  assert.equal(
    filespaceViewerKind({ name: "noext", mime: "text/plain" }),
    "text",
  );
  assert.equal(
    filespaceViewerKind({ name: "weird", category: "code" }),
    "text",
  );
});
