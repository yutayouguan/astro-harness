import { test } from "node:test";
import assert from "node:assert/strict";
import {
  createAmbienceSession,
  appearanceKey,
  appearanceEditUndo,
  type AmbienceAppearance,
} from "./ambienceSession.ts";
import { DEFAULT_WALLPAPER_PREFS } from "./wallpaper.ts";
import { DEFAULT_SHELL_COLOR_PREFS } from "./shellGradient.ts";
const checkpoint = {
  kind: "native" as const,
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

const appearance: AmbienceAppearance = {
  material: "soft",
  mode: "auto",
  glassIntensity: 64,
  softFrostIntensity: 50,
};
test("one continuous material gesture restores its initial value", () => {
  const middle = { ...appearance, softFrostIntensity: 60 };
  const end = { ...appearance, softFrostIntensity: 90 };
  const first = appearanceEditUndo(appearance, middle, null);
  const last = appearanceEditUndo(middle, end, first);
  assert.deepEqual(last.before, appearance);
  assert.equal(last.expected, appearanceKey(end));
  const session = createAmbienceSession();
  session.setUndo(last);
  assert.equal(session.getSnapshot().undo?.kind, "appearance");
});
test("external appearance edits break gesture coalescing rather than being overwritten", () => {
  const middle = { ...appearance, softFrostIntensity: 60 };
  const external = { ...middle, mode: "dark" as const };
  const last = appearanceEditUndo(
    external,
    { ...external, softFrostIntensity: 90 },
    appearanceEditUndo(appearance, middle, null),
  );
  assert.deepEqual(last.before, external);
});
test("a native change replaces appearance undo and vice versa", () => {
  const session = createAmbienceSession();
  session.setUndo(
    appearanceEditUndo(appearance, { ...appearance, material: "glass" }, null),
  );
  session.setUndo(checkpoint);
  assert.equal(session.getSnapshot().undo?.kind, "native");
});
