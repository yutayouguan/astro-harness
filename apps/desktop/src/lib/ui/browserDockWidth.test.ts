import assert from "node:assert/strict";
import { test } from "node:test";
import {
  BROWSER_DOCK_DEFAULT_WIDTH,
  BROWSER_DOCK_MAX_WIDTH,
  BROWSER_DOCK_MIN_WIDTH,
  clampBrowserDockWidth,
  maxBrowserDockWidth,
  parseStoredBrowserDockWidth,
} from "./browserDockWidth.ts";

test("parses a stored browser dock width with safe fallbacks", () => {
  assert.equal(parseStoredBrowserDockWidth(null), BROWSER_DOCK_DEFAULT_WIDTH);
  assert.equal(
    parseStoredBrowserDockWidth("not-a-number"),
    BROWSER_DOCK_DEFAULT_WIDTH,
  );
  assert.equal(parseStoredBrowserDockWidth("760"), 760);
  assert.equal(parseStoredBrowserDockWidth("200"), BROWSER_DOCK_MIN_WIDTH);
  assert.equal(parseStoredBrowserDockWidth("1600"), BROWSER_DOCK_MAX_WIDTH);
});

test("keeps the main chat usable and lets narrow layouts use an overlay", () => {
  assert.equal(maxBrowserDockWidth(1_000), 680);
  assert.equal(clampBrowserDockWidth(900, 1_000), 680);
  assert.equal(clampBrowserDockWidth(200, 1_000), BROWSER_DOCK_MIN_WIDTH);
  assert.equal(maxBrowserDockWidth(800), 480);
  assert.equal(maxBrowserDockWidth(800, true), 800);
  assert.equal(clampBrowserDockWidth(760, 800, true), 760);
});
