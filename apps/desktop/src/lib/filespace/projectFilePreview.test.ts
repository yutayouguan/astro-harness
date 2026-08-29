import assert from "node:assert/strict";
import test from "node:test";
import { projectFileOpenPlan } from "./projectFilePreview.ts";

test("projectFileOpenPlan keeps editable formats on the UTF-8 reader", () => {
  assert.deepEqual(projectFileOpenPlan("README.md"), {
    kind: "markdown",
    readAsText: true,
    readonly: false,
  });
  assert.deepEqual(projectFileOpenPlan("main.rs"), {
    kind: "text",
    readAsText: true,
    readonly: false,
  });
  assert.deepEqual(projectFileOpenPlan("index.html"), {
    kind: "html",
    readAsText: true,
    readonly: false,
  });
});

test("projectFileOpenPlan bypasses the UTF-8 reader for previews and external files", () => {
  for (const [name, kind] of [
    ["cover.png", "image"],
    ["clip.mp4", "video"],
    ["voice.m4a", "audio"],
    ["report.pdf", "pdf"],
    ["slides.pptx", "external"],
  ] as const) {
    assert.deepEqual(projectFileOpenPlan(name), {
      kind,
      readAsText: false,
      readonly: true,
    });
  }
});
