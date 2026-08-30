import assert from "node:assert/strict";
import { test } from "node:test";
import { resolveChatRightDock } from "./chatRightDock.ts";

test("resolves conflicting right-panel flags to one dock surface", () => {
  assert.equal(
    resolveChatRightDock({
      projectFilesOpen: true,
      sideSessionOpen: true,
      inspectorOpen: true,
      reviewOpen: false,
    }),
    "project-files",
  );
  assert.equal(
    resolveChatRightDock({
      projectFilesOpen: false,
      sideSessionOpen: true,
      inspectorOpen: true,
      reviewOpen: false,
    }),
    "side-chat",
  );
  assert.equal(
    resolveChatRightDock({
      projectFilesOpen: false,
      sideSessionOpen: false,
      inspectorOpen: true,
      reviewOpen: false,
    }),
    "inspector",
  );
  assert.equal(
    resolveChatRightDock({
      projectFilesOpen: false,
      sideSessionOpen: false,
      inspectorOpen: false,
      reviewOpen: false,
    }),
    null,
  );
});

test("review takes precedence over stale dock flags", () => {
  assert.equal(
    resolveChatRightDock({
      projectFilesOpen: true,
      sideSessionOpen: true,
      inspectorOpen: true,
      reviewOpen: true,
    }),
    "review",
  );
});
