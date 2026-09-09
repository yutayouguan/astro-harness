import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import {
  canConfirmCleanup,
  cleanupCopy,
  parseCleanupPlan,
  parseCleanupResult,
  type CleanupPlan,
} from "./storageCleanup.ts";

const plan: CleanupPlan = {
  token: "00000000-0000-4000-8000-000000000001",
  rootPath: "/fixture",
  expiresAtMs: 2000,
  items: [
    { path: "models/cache/owned.json", bytes: 123, policy: "cache_expired" },
  ],
  totalBytes: 123,
  omittedFiles: false,
};
test("confirmation requires acknowledgement and an unexpired non-busy plan", () => {
  assert.equal(canConfirmCleanup(plan, false, false, 1000), false);
  assert.equal(canConfirmCleanup(plan, true, true, 1000), false);
  assert.equal(canConfirmCleanup(plan, true, false, 2000), false);
  assert.equal(canConfirmCleanup(plan, true, false, 1000), true);
});
test("plan parser refuses traversal duplicate paths totals and oversized lists", () => {
  assert.deepEqual(parseCleanupPlan(plan), plan);
  assert.throws(() =>
    parseCleanupPlan({
      ...plan,
      items: [{ ...plan.items[0], path: "models/\u202ecache/file" }],
    }),
  );
  for (const value of [
    null,
    {},
    { ...plan, totalBytes: 1 },
    { ...plan, items: [plan.items[0], plan.items[0]], totalBytes: 246 },
    { ...plan, items: [{ ...plan.items[0], path: "../state.db" }] },
    { ...plan, items: Array(21).fill(plan.items[0]) },
  ]) {
    assert.throws(() => parseCleanupPlan(value));
  }
});
test("results distinguish verified moves from changed recovery files", () => {
  const result = {
    batchId: plan.token,
    recoveryPath: "/fixture/backups/batch",
    movedFiles: 0,
    movedBytes: 0,
    unverifiedFiles: 1,
    manifestComplete: true,
    outcomes: [{ path: plan.items[0].path, status: "changed_in_recovery" }],
  };
  assert.deepEqual(parseCleanupResult(result), result);
  assert.throws(() =>
    parseCleanupResult({ ...result, batchId: "../../escape" }),
  );
});
test("cleanup copy describes recovery rather than immediate space reclamation", () => {
  assert.match(cleanupCopy.zh.explanation, /磁盘空间不会立即释放/);
  assert.deepEqual(
    Object.keys(cleanupCopy.zh.errors),
    Object.keys(cleanupCopy.en.errors),
  );
  assert.deepEqual(
    Object.keys(cleanupCopy.zh.statuses),
    Object.keys(cleanupCopy.en.statuses),
  );
});
test("mutation sends only the token and explicit confirmation, not file paths", () => {
  const source = readFileSync(
    new URL("../../components/settings/StorageCleanup.tsx", import.meta.url),
    "utf8",
  );
  assert.match(source, /confirmOnEnter=\{false\}/);
  assert.match(source, /trapFocus/);
  assert.match(source, /confirmed: true/);
  assert.match(source, /canConfirmCleanup/);
  assert.match(source, /discard_storage_cleanup/);
  assert.doesNotMatch(source, /delete_file|remove_file|empty_trash/);
});
