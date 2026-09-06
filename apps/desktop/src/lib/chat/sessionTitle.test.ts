import assert from "node:assert/strict";
import test from "node:test";
import { visibleSessionTitle } from "./sessionTitle.ts";

test("hides the generated short id from duplicate cron session titles", () => {
  assert.equal(
    visibleSessionTitle(
      "定时任务 · test · 737536f4",
      "737536f4-6588-4cbc-99a2-7abc20d49b15",
    ),
    "定时任务 · test",
  );
});

test("keeps normal titles and unrelated suffixes intact", () => {
  assert.equal(
    visibleSessionTitle("test · 737536f4", "737536f4-rest"),
    "test · 737536f4",
  );
  assert.equal(
    visibleSessionTitle("定时任务 · test · abcdef12", "737536f4-rest"),
    "定时任务 · test · abcdef12",
  );
});
