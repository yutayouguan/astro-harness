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

test("weekly shortcut and custom weekly schedules round-trip", () => {
  const weekly = decodeSchedule(
    encodeSchedule({ mode: "weekly", time: "16:00", weekdays: [5] }),
  );
  assert.deepEqual(weekly, { mode: "weekly", time: "16:00", weekdays: [5] });

  const custom = decodeSchedule(
    encodeSchedule({
      mode: "custom",
      customFrequency: "weekly",
      customInterval: 1,
      time: "08:30",
      weekdays: [1, 3, 6],
    }),
  );
  assert.deepEqual(custom, {
    mode: "custom",
    customFrequency: "weekly",
    customInterval: 1,
    time: "08:30",
    weekdays: [1, 3, 6],
  });
});

test("custom calendar frequencies round-trip with their adaptive fields", () => {
  const drafts = [
    {
      mode: "custom",
      customFrequency: "hourly",
      customInterval: 2,
      minute: 15,
      weekdays: [],
    },
    {
      mode: "custom",
      customFrequency: "daily",
      customInterval: 3,
      time: "08:20",
      weekdays: [],
    },
    {
      mode: "custom",
      customFrequency: "monthly",
      customInterval: 2,
      monthDay: 18,
      time: "10:30",
      weekdays: [],
    },
    {
      mode: "custom",
      customFrequency: "yearly",
      customInterval: 1,
      month: 1,
      monthDay: 1,
      time: "08:00",
      weekdays: [],
    },
  ];

  for (const draft of drafts) {
    assert.deepEqual(decodeSchedule(encodeSchedule(draft)), draft);
  }
});

test("standard monthly and yearly cron expressions open in custom mode", () => {
  assert.deepEqual(decodeSchedule("0 8 15 * *"), {
    mode: "custom",
    customFrequency: "monthly",
    customInterval: 1,
    monthDay: 15,
    time: "08:00",
    weekdays: [],
  });
  assert.deepEqual(decodeSchedule("0 8 1 1 *"), {
    mode: "custom",
    customFrequency: "yearly",
    customInterval: 1,
    month: 1,
    monthDay: 1,
    time: "08:00",
    weekdays: [],
  });
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
