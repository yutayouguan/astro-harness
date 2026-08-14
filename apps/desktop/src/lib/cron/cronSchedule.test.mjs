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
  assert.equal(d?.mode, "daily");
  assert.equal(d?.time, "08:30");
  assert.deepEqual(d?.weekdays, [1, 3, 5]);
});

test("decode every interval", () => {
  const d = decodeSchedule("every:5m");
  assert.deepEqual(d, {
    mode: "interval",
    time: "09:00",
    weekdays: [],
    intervalValue: 5,
    intervalUnit: "m",
  });
});

test("decode every with weekday filter", () => {
  const d = decodeSchedule("every:1h;wd=1-5");
  assert.equal(d?.mode, "interval");
  assert.equal(d?.intervalValue, 1);
  assert.equal(d?.intervalUnit, "h");
  assert.deepEqual(d?.weekdays, [1, 2, 3, 4, 5]);
});

test("decode once", () => {
  const d = decodeSchedule("once:2026-07-12T15:30:00+08:00");
  assert.equal(d?.mode, "once");
  assert.equal(d?.onceAt, "2026-07-12T15:30");
});

test("round-trip daily encode/decode", () => {
  const draft = {
    mode: "daily",
    time: "09:00",
    weekdays: [1, 2, 3, 4, 5],
  };
  const encoded = encodeSchedule(draft);
  const decoded = decodeSchedule(encoded);
  assert.equal(decoded?.mode, "daily");
  assert.equal(decoded?.time, "09:00");
  assert.deepEqual(decoded?.weekdays, [1, 2, 3, 4, 5]);
});

test("decode daily with weekday range", () => {
  const d = decodeSchedule("0 9 * * 1-5");
  assert.equal(d?.mode, "daily");
  assert.equal(d?.time, "09:00");
  assert.deepEqual(d?.weekdays, [1, 2, 3, 4, 5]);
});

test("unknown schedule returns null", () => {
  assert.equal(decodeSchedule("sometime"), null);
  assert.equal(decodeSchedule(""), null);
});
