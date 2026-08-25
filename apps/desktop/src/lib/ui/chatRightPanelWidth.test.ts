import { test } from "node:test";
import assert from "node:assert/strict";
import {
  CHAT_RIGHT_PANEL_DEFAULT_WIDTH,
  CHAT_RIGHT_PANEL_MAX_WIDTH,
  CHAT_RIGHT_PANEL_MIN_WIDTH,
  clampChatRightPanelWidth,
  maxChatRightPanelWidth,
  parseStoredChatRightPanelWidth,
} from "./chatRightPanelWidth.ts";

test("parses a stored right-panel width with safe fallbacks", () => {
  assert.equal(parseStoredChatRightPanelWidth(null), CHAT_RIGHT_PANEL_DEFAULT_WIDTH);
  assert.equal(parseStoredChatRightPanelWidth("not-a-number"), CHAT_RIGHT_PANEL_DEFAULT_WIDTH);
  assert.equal(parseStoredChatRightPanelWidth("480"), 480);
  assert.equal(parseStoredChatRightPanelWidth("120"), CHAT_RIGHT_PANEL_MIN_WIDTH);
  assert.equal(parseStoredChatRightPanelWidth("900"), CHAT_RIGHT_PANEL_MAX_WIDTH);
});

test("clamps the right panel to the current layout width", () => {
  assert.equal(maxChatRightPanelWidth(1000), CHAT_RIGHT_PANEL_MAX_WIDTH);
  assert.equal(clampChatRightPanelWidth(640, 500), 480);
  assert.equal(clampChatRightPanelWidth(360, 300), 280);
  assert.equal(clampChatRightPanelWidth(200, 300), 280);
});
