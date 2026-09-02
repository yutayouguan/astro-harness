import assert from "node:assert/strict";
import { test } from "node:test";
import { nextBrowserPreviewRevision } from "./browserPreviewState.ts";

test("browser preview revisions remain monotonic within the same millisecond", () => {
  assert.equal(nextBrowserPreviewRevision(1_000, 1_000), 1_001);
  assert.equal(nextBrowserPreviewRevision(999, 1_000), 1_000);
  assert.equal(nextBrowserPreviewRevision(undefined, 1_000), 1_000);
});
