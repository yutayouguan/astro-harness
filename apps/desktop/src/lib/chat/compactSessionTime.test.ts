import assert from "node:assert/strict";
import test from "node:test";

import { compactSessionTime } from "./compactSessionTime.ts";

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;
const now = new Date(2026, 8, 7, 12, 0, 0).getTime();
const ago = (duration: number) => new Date(now - duration).toISOString();

test("compact session time uses progressively shorter units", () => {
  assert.equal(compactSessionTime(ago(20_000), "刚刚", now), "刚刚");
  assert.equal(compactSessionTime(ago(12 * MINUTE_MS), "刚刚", now), "12m");
  assert.equal(compactSessionTime(ago(7 * HOUR_MS), "刚刚", now), "7h");
  assert.equal(compactSessionTime(ago(3 * DAY_MS), "刚刚", now), "3d");
});

test("sessions at least seven days old use a compact local date", () => {
  const created = new Date(now - 8 * DAY_MS);
  const expected = `${String(created.getMonth() + 1).padStart(2, "0")}/${String(
    created.getDate(),
  ).padStart(2, "0")}`;
  assert.equal(compactSessionTime(created.toISOString(), "刚刚", now), expected);
});

test("invalid and future timestamps fail closed", () => {
  assert.equal(compactSessionTime(null, "刚刚", now), "");
  assert.equal(compactSessionTime("bad timestamp", "刚刚", now), "");
  assert.equal(
    compactSessionTime(new Date(now + HOUR_MS).toISOString(), "刚刚", now),
    "刚刚",
  );
});
