import assert from "node:assert/strict";
import test from "node:test";
import {
  CRON_SESSION_ID_PREFIX,
  cronJobIdFromSessionId,
  cronOwnerNeedsAttention,
  resolveCronOwnerStates,
} from "./cronSessionOwner.ts";

test("derives the job id from a cron session id", () => {
  assert.equal(CRON_SESSION_ID_PREFIX, "cron-");
  assert.equal(cronJobIdFromSessionId("cron-job-a"), "job-a");
  assert.equal(cronJobIdFromSessionId("  cron-job-a  "), "job-a");
  assert.equal(cronJobIdFromSessionId("cron-"), null);
  assert.equal(cronJobIdFromSessionId("737536f4-6588-4cbc"), null);
});

test("classifies cron sessions by their owning job", () => {
  const states = resolveCronOwnerStates(
    ["cron-job-active", "cron-job-archived", "cron-job-gone", "plain-session"],
    [
      { id: "job-active", archived_at: null },
      { id: "job-archived", archived_at: "2026-09-27T00:00:00Z" },
    ],
  );

  assert.equal(states.get("cron-job-active"), "active");
  assert.equal(states.get("cron-job-archived"), "archived");
  assert.equal(states.get("cron-job-gone"), "missing");
  // 普通会话不参与归属统计
  assert.equal(states.has("plain-session"), false);
});

test("treats legacy jobs without archived_at as active", () => {
  const states = resolveCronOwnerStates(
    ["cron-job-legacy"],
    [{ id: "job-legacy" }],
  );
  assert.equal(states.get("cron-job-legacy"), "active");
});

test("only archived and missing owners need a sidebar hint", () => {
  assert.equal(cronOwnerNeedsAttention("missing"), true);
  assert.equal(cronOwnerNeedsAttention("archived"), true);
  assert.equal(cronOwnerNeedsAttention("active"), false);
  assert.equal(cronOwnerNeedsAttention(undefined), false);
});
