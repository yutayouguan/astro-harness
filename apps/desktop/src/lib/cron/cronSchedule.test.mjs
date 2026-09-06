import assert from "node:assert/strict";
import test from "node:test";
import {
  decodeSchedule,
  encodeSchedule,
  formatScheduleLabel,
} from "./cronSchedule.ts";

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

test("invalid intervals fall back without capping valid large values", () => {
  const fallback = { mode: "daily", time: "09:00", weekdays: [] };
  assert.deepEqual(decodeSchedule("every:0h"), fallback);
  assert.deepEqual(decodeSchedule("every:2.5h"), fallback);
  assert.deepEqual(decodeSchedule("every:2h;wd=9"), fallback);
  assert.equal(decodeSchedule("every:1000d").intervalValue, 1000);
  assert.equal(
    encodeSchedule({
      mode: "interval",
      intervalValue: 5000,
      intervalUnit: "h",
      weekdays: [],
    }),
    "every:5000h",
  );
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
      start: "2026-07-11T10:10",
      weekdays: [1, 3, 6],
    }),
  );
  assert.deepEqual(custom, {
    mode: "custom",
    customFrequency: "weekly",
    customInterval: 1,
    time: "08:30",
    start: "2026-07-11T10:10",
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
      start: "2026-07-11T10:10",
      weekdays: [],
    },
    {
      mode: "custom",
      customFrequency: "daily",
      customInterval: 3,
      time: "08:20",
      start: "2026-07-11T10:10",
      weekdays: [],
    },
    {
      mode: "custom",
      customFrequency: "monthly",
      customInterval: 2,
      monthDay: 18,
      time: "10:30",
      start: "2026-07-11T10:10",
      weekdays: [],
    },
    {
      mode: "custom",
      customFrequency: "yearly",
      customInterval: 1,
      month: 1,
      monthDay: 1,
      time: "08:00",
      start: "2026-07-11T10:10",
      weekdays: [],
    },
  ];

  for (const draft of drafts) {
    assert.deepEqual(decodeSchedule(encodeSchedule(draft)), draft);
  }
});

test("custom encoding bounds values and persists a valid start phase", () => {
  assert.equal(
    encodeSchedule({
      mode: "custom",
      customFrequency: "yearly",
      customInterval: 10_000,
      month: 2,
      monthDay: 31,
      time: "99:99",
      start: "2026-02-30T99:99",
      weekdays: [],
    }).match(
      /^custom:yearly;every=999;month=2;day=29;time=09:00;start=\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/,
    ) !== null,
    true,
  );
});

test("invalid expressions do not masquerade as valid custom schedules", () => {
  const fallback = { mode: "daily", time: "09:00", weekdays: [] };
  assert.deepEqual(
    decodeSchedule("custom:daily;every=2.5;time=08:00"),
    fallback,
  );
  assert.deepEqual(decodeSchedule("custom:daily;every=2;time=99:99"), fallback);
  assert.deepEqual(
    decodeSchedule("custom:hourly;every=2;time=08:00"),
    fallback,
  );
  assert.deepEqual(
    decodeSchedule("custom:daily;every=2;time=08:00;start=2026-02-30T10:10"),
    fallback,
  );
  assert.deepEqual(decodeSchedule("99 99 * * *"), fallback);
  assert.deepEqual(decodeSchedule("0 8 31 2 *"), fallback);
});

test("custom decoder matches backend whitespace and weekday-range parsing", () => {
  assert.deepEqual(
    decodeSchedule("custom:weekly ; every=2 ; wd=1-5 ; time=08:00"),
    {
      mode: "custom",
      customFrequency: "weekly",
      customInterval: 2,
      time: "08:00",
      weekdays: [1, 2, 3, 4, 5],
    },
  );
});

test("monthly, yearly, and hourly cron labels describe the actual cadence", () => {
  assert.equal(formatScheduleLabel("30 * * * *", "zh"), "每小时第 30 分");
  assert.equal(formatScheduleLabel("0 8 15 * *", "zh"), "每月 15日 08:00");
  assert.equal(formatScheduleLabel("0 8 1 1 *", "zh"), "每年 1月1日 08:00");
  assert.equal(
    formatScheduleLabel("0 8 15 * *", "en"),
    "Monthly on day 15 at 08:00",
  );
  assert.equal(formatScheduleLabel("0 8 * * 1-5", "zh"), "工作日 08:00");
  assert.equal(formatScheduleLabel("0 8 31 2 *", "zh"), "0 8 31 2 *");
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
