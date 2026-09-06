import assert from "node:assert/strict";
import test from "node:test";
import { buildMonthTimeline, countMonthEntries } from "./memoryTimeline.ts";

test("countMonthEntries counts diary entries across experts, not unique days", () => {
  const count = countMonthEntries(
    [
      ["2026-09-06", "2026-08-31"],
      ["2026-09-06", "2026-09-03"],
    ],
    "2026-09-",
  );

  assert.equal(count, 3);
});

test("buildMonthTimeline merges diary and dream days in reverse date order", () => {
  const timeline = buildMonthTimeline({
    monthPrefix: "2026-09-",
    diaryDates: new Set(["2026-09-03", "2026-09-06", "2026-08-31"]),
    dreamDates: new Set(["2026-09-04", "2026-09-06"]),
    diaryDatesByAgent: {
      default: ["2026-09-06", "2026-09-03"],
      writer: ["2026-09-06"],
    },
    includeAgentCount: true,
  });

  assert.deepEqual(timeline, [
    {
      date: "2026-09-06",
      hasDiary: true,
      hasDream: true,
      diaryAgentCount: 2,
    },
    {
      date: "2026-09-04",
      hasDiary: false,
      hasDream: true,
      diaryAgentCount: 0,
    },
    {
      date: "2026-09-03",
      hasDiary: true,
      hasDream: false,
      diaryAgentCount: 1,
    },
  ]);
});

test("buildMonthTimeline uses a single diary owner outside all-experts mode", () => {
  const [item] = buildMonthTimeline({
    monthPrefix: "2026-09-",
    diaryDates: new Set(["2026-09-06"]),
    dreamDates: new Set(),
    diaryDatesByAgent: {
      default: ["2026-09-06"],
      writer: ["2026-09-06"],
    },
    includeAgentCount: false,
  });

  assert.equal(item?.diaryAgentCount, 1);
});
