import assert from "node:assert/strict";
import test from "node:test";
import {
  isLiveActivityStatus,
  isSettledActivityStatus,
  resolveToolActivityStatus,
} from "./toolActivityStatus.ts";

test("maps protocol tool phases onto stable activity states", () => {
  assert.equal(resolveToolActivityStatus("started", undefined), "running");
  assert.equal(
    resolveToolActivityStatus("pending_approval", undefined),
    "waiting",
  );
  assert.equal(resolveToolActivityStatus("retrying", undefined), "retrying");
  assert.equal(resolveToolActivityStatus("partial", undefined), "partial");
  assert.equal(resolveToolActivityStatus("completed", undefined), "done");
  assert.equal(resolveToolActivityStatus("failed", undefined), "error");
  assert.equal(resolveToolActivityStatus("declined", undefined), "declined");
  assert.equal(
    resolveToolActivityStatus("cancelled", undefined),
    "interrupted",
  );
});

test("uses a returned result as completion fallback", () => {
  assert.equal(resolveToolActivityStatus(undefined, "ok"), "done");
  assert.equal(resolveToolActivityStatus(undefined, ""), "running");
});

test("only waiting, running, and retrying states remain live", () => {
  assert.equal(isLiveActivityStatus("waiting"), true);
  assert.equal(isLiveActivityStatus("running"), true);
  assert.equal(isLiveActivityStatus("retrying"), true);
  assert.equal(isLiveActivityStatus("partial"), false);
  assert.equal(isLiveActivityStatus("error"), false);
  assert.equal(isSettledActivityStatus("done"), true);
  assert.equal(isSettledActivityStatus("partial"), true);
  assert.equal(isSettledActivityStatus("error"), true);
  assert.equal(isSettledActivityStatus("declined"), true);
  assert.equal(isSettledActivityStatus("interrupted"), true);
  assert.equal(isSettledActivityStatus("running"), false);
});
