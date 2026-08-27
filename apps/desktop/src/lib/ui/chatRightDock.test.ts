import assert from "node:assert/strict";
import { test } from "node:test";
import {
  CHAT_SIDE_PANEL_WIDTH,
  chatRightDockWidth,
  resolveChatRightDock,
} from "./chatRightDock.ts";

test("resolves conflicting right-panel flags to one dock surface", () => {
  assert.equal(
    resolveChatRightDock({
      projectFilesOpen: true,
      sideSessionOpen: true,
      inspectorOpen: true,
    }),
    "project-files",
  );
  assert.equal(
    resolveChatRightDock({
      projectFilesOpen: false,
      sideSessionOpen: true,
      inspectorOpen: true,
    }),
    "side-chat",
  );
  assert.equal(
    resolveChatRightDock({
      projectFilesOpen: false,
      sideSessionOpen: false,
      inspectorOpen: true,
    }),
    "inspector",
  );
  assert.equal(
    resolveChatRightDock({
      projectFilesOpen: false,
      sideSessionOpen: false,
      inspectorOpen: false,
    }),
    null,
  );
});

test("header clearance follows only the active dock width", () => {
  const widths = { projectFiles: 280, inspector: 420 };
  assert.equal(chatRightDockWidth("project-files", widths), 280);
  assert.equal(chatRightDockWidth("side-chat", widths), CHAT_SIDE_PANEL_WIDTH);
  assert.equal(chatRightDockWidth("inspector", widths), 420);
  assert.equal(chatRightDockWidth(null, widths), 0);
});
