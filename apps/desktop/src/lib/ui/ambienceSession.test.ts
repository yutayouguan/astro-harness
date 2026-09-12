import { test } from "node:test";
import assert from "node:assert/strict";
import { createAmbienceSession } from "./ambienceSession.ts";
import { DEFAULT_WALLPAPER_PREFS } from "./wallpaper.ts";
import { DEFAULT_SHELL_COLOR_PREFS } from "./shellGradient.ts";
const checkpoint = {
  token: "native-token",
  wallpaper: DEFAULT_WALLPAPER_PREFS,
  colors: DEFAULT_SHELL_COLOR_PREFS,
  expected: "after",
};

test("unsubscribing a panel preserves its undo for a remount", () => {
  const session = createAmbienceSession();
  let events = 0;
  const leave = session.subscribe(() => events++);
  session.setUndo(checkpoint);
  leave();
  assert.equal(session.getSnapshot().undo, checkpoint);
  let nextEvents = 0;
  session.subscribe(() => nextEvents++);
  session.setUndo(null);
  assert.equal(events, 1);
  assert.equal(nextEvents, 1);
});
test("pending changes remain mutually exclusive while navigating", () => {
  const session = createAmbienceSession();
  assert.equal(session.begin(), true);
  const leave = session.subscribe(() => {});
  leave();
  assert.equal(session.begin(), false);
  session.setUndo(checkpoint);
  session.finish();
  assert.equal(session.getSnapshot().undo, checkpoint);
  assert.equal(session.begin(), true);
  session.finish();
});
test("a new window session cannot reuse another process's undo token", () => {
  const original = createAmbienceSession();
  original.setUndo(checkpoint);
  assert.equal(createAmbienceSession().getSnapshot().undo, null);
});
test("snapshot identity stays stable until a change for useSyncExternalStore", () => {
  const session = createAmbienceSession();
  const first = session.getSnapshot();
  assert.equal(first, session.getSnapshot());
  session.begin();
  assert.notEqual(first, session.getSnapshot());
});
