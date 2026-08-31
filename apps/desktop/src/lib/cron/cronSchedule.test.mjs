import assert from "node:assert/strict";
import test from "node:test";
import { decodeSchedule, encodeSchedule } from "./cronSchedule.ts";

test("decode daily five-field cron", () => {
  const d = decodeSchedule("0 9 * * *");
  assert.deepEqual(d, {
    mode: "daily",
    time: "09:00",
    weekdays: [],
  });
});

test("decode daily with weekdays", () => {
  const d = decodeSchedule("30 8 * * 1,3,5");
  assert.equal(d?.mode, "custom");
  assert.equal(d?.time, "08:30");
  assert.deepEqual(d?.weekdays, [1, 3, 5]);
});

test("decode every interval", () => {
  const d = decodeSchedule("every:5m");
  assert.deepEqual(d, {
    mode: "interval",
    weekdays: [],
    intervalValue: 5,
    intervalUnit: "m",
  });
});

test("decode day interval", () => {
  const d = decodeSchedule("every:2d");
  assert.deepEqual(d, {
    mode: "interval",
    weekdays: [],
    intervalValue: 2,
    intervalUnit: "d",
  });
});

test("decode every with weekday filter", () => {
  const d = decodeSchedule("every:1h;wd=1-5");
  assert.equal(d?.mode, "interval");
  assert.equal(d?.intervalValue, 1);
  assert.equal(d?.intervalUnit, "h");
  assert.deepEqual(d?.weekdays, [1, 2, 3, 4, 5]);
});

test("legacy once schedule remains round-trippable without being a creation mode", () => {
  const d = decodeSchedule("once:2026-07-12T15:30:00+08:00");
  assert.equal(d?.mode, "once");
  assert.equal(d?.onceAt, "2026-07-12T15:30");
  assert.match(encodeSchedule(d), /^once:2026-07-12T15:30:00\+08:00$/);
});

test("round-trip weekdays encode/decode", () => {
  const draft = {
    mode: "weekdays",
    time: "09:00",
    weekdays: [1, 2, 3, 4, 5],
  };
  const encoded = encodeSchedule(draft);
  const decoded = decodeSchedule(encoded);
  assert.equal(decoded?.mode, "weekdays");
  assert.equal(decoded?.time, "09:00");
  assert.deepEqual(decoded?.weekdays, [1, 2, 3, 4, 5]);
});

test("decode daily with weekday range", () => {
  const d = decodeSchedule("0 9 * * 1-5");
  assert.equal(d?.mode, "weekdays");
  assert.equal(d?.time, "09:00");
  assert.deepEqual(d?.weekdays, [1, 2, 3, 4, 5]);
});

test("weekly and custom modes round-trip through five-field cron", () => {
  const weekly = decodeSchedule(
    encodeSchedule({ mode: "weekly", time: "16:00", weekdays: [5] }),
  );
  assert.deepEqual(weekly, { mode: "weekly", time: "16:00", weekdays: [5] });

  const custom = decodeSchedule(
    encodeSchedule({ mode: "custom", time: "08:30", weekdays: [1, 3, 6] }),
  );
  assert.deepEqual(custom, { mode: "custom", time: "08:30", weekdays: [1, 3, 6] });
});

test("daily ignores stale weekday selections", () => {
  assert.equal(
    encodeSchedule({ mode: "daily", time: "07:45", weekdays: [1, 2, 3] }),
    "45 7 * * *",
  );
});

test("unknown schedule falls back to a safe daily draft", () => {
  const fallback = { mode: "daily", time: "09:00", weekdays: [] };
  assert.deepEqual(decodeSchedule("sometime"), fallback);
  assert.deepEqual(decodeSchedule(""), fallback);
});
