import assert from "node:assert/strict";
import test from "node:test";
import { displayUserPath } from "./displayPath.ts";

test("keeps tilde paths and normalizes slashes", () => {
  assert.equal(displayUserPath("~/Downloads/a.png"), "~/Downloads/a.png");
  assert.equal(
    displayUserPath("C:\\Users\\me\\Downloads\\a.png"),
    "~/Downloads/a.png",
  );
});

test("maps common home prefixes", () => {
  assert.equal(
    displayUserPath("/Users/alice/Downloads/x.png"),
    "~/Downloads/x.png",
  );
  assert.equal(
    displayUserPath("/home/bob/Downloads/x.png"),
    "~/Downloads/x.png",
  );
  assert.equal(
    displayUserPath("C:/Users/carol/Downloads/x.png"),
    "~/Downloads/x.png",
  );
});
