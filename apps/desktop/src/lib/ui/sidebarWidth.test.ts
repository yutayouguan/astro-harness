import { test } from "node:test";
import assert from "node:assert/strict";
import {
  SIDEBAR_COMPACT_MAX_WIDTH,
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_MIN_WIDTH,
  clampSidebarWidth,
  maxSidebarWidth,
  parseStoredSidebarWidth,
  shouldUseCompactSidebar,
} from "./sidebarWidth.ts";

test("parses the stored project sidebar width safely", () => {
  assert.equal(SIDEBAR_DEFAULT_WIDTH, 280);
  assert.equal(parseStoredSidebarWidth(null), SIDEBAR_DEFAULT_WIDTH);
  assert.equal(parseStoredSidebarWidth("invalid"), SIDEBAR_DEFAULT_WIDTH);
  assert.equal(parseStoredSidebarWidth("320"), 320);
  assert.equal(parseStoredSidebarWidth("120"), SIDEBAR_MIN_WIDTH);
  assert.equal(parseStoredSidebarWidth("800"), SIDEBAR_MAX_WIDTH);
});

test("keeps enough room for the main content on narrow windows", () => {
  assert.equal(maxSidebarWidth(1100), SIDEBAR_MAX_WIDTH);
  assert.equal(clampSidebarWidth(400, 780), 340);
  assert.equal(clampSidebarWidth(240, 620), 180);
  assert.equal(clampSidebarWidth(120, 620), 180);
});

test("switches the sidebar to icon-only density at the compact breakpoint", () => {
  assert.equal(SIDEBAR_COMPACT_MAX_WIDTH, 760);
  assert.equal(shouldUseCompactSidebar(761), false);
  assert.equal(shouldUseCompactSidebar(760), true);
  assert.equal(shouldUseCompactSidebar(480), true);
  assert.equal(shouldUseCompactSidebar(0), false);
  assert.equal(shouldUseCompactSidebar(Number.POSITIVE_INFINITY), false);
});
