import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import {
  parseStorageReport,
  storageBytes,
  storageTotals,
  storageCopy,
  type StorageReport,
} from "./storageDiagnostics.ts";

const report: StorageReport = {
  rootPath: "/fixture",
  configPath: "/fixture/config.toml",
  state: "ready",
  settingsVersion: 1,
  configPresent: true,
  partial: false,
  inspectedEntries: 3,
  issues: [],
  cleanupPreview: [],
  previewPartial: false,
  cachePolicies: [],
  domains: [
    {
      id: "models",
      bytes: 1024,
      files: 2,
      skippedLinks: 1,
      previewBytes: 100,
      previewFiles: 1,
    },
    {
      id: "sessions",
      bytes: 2048,
      files: 1,
      skippedLinks: 0,
      previewBytes: 0,
      previewFiles: 0,
    },
  ],
};

test("storage reports validate metadata without requiring configuration contents", () => {
  assert.deepEqual(parseStorageReport(report), report);
  assert.throws(
    () => parseStorageReport({ ...report, cachePolicies: [{}] }),
    /invalid_storage_report/,
  );
  assert.throws(
    () => parseStorageReport({ ...report, previewPartial: undefined }),
    /invalid_storage_report/,
  );
  for (const invalid of [
    [],
    null,
    {},
    { ...report, state: "unexpected" },
    { ...report, domains: [{ bytes: -1 }] },
  ]) {
    assert.throws(() => parseStorageReport(invalid), /invalid_storage_report/);
  }
});
test("totals separate protected bytes from preview candidates", () => {
  assert.deepEqual(storageTotals(report), {
    bytes: 3072,
    files: 3,
    previewBytes: 100,
    previewFiles: 1,
    skippedLinks: 1,
  });
});
test("byte formatting is bounded and handles zero and invalid input", () => {
  assert.equal(storageBytes(0), "0 B");
  assert.equal(storageBytes(NaN), "0 B");
  assert.equal(storageBytes(1024), "1 KiB");
  assert.equal(storageBytes(1048576), "1 MiB");
  assert.ok(!storageBytes(0.5).includes("undefined"));
});
test("every diagnostic status and issue has matching Chinese and English copy", () => {
  assert.deepEqual(
    Object.keys(storageCopy.zh.policyStates),
    Object.keys(storageCopy.en.policyStates),
  );
  assert.deepEqual(
    Object.keys(storageCopy.zh.states),
    Object.keys(storageCopy.en.states),
  );
  assert.deepEqual(
    Object.keys(storageCopy.zh.issueNames),
    Object.keys(storageCopy.en.issueNames),
  );
  assert.deepEqual(
    Object.keys(storageCopy.zh.domainNames),
    Object.keys(storageCopy.en.domainNames),
  );
});
test("inspection stays read-only and delegates mutations to explicit confirmation", () => {
  const source = readFileSync(
    new URL(
      "../../components/settings/StorageDiagnostics.tsx",
      import.meta.url,
    ),
    "utf8",
  );
  assert.match(source, /invoke\("inspect_home_storage"/);
  assert.match(source, /<StorageCleanup/);
  assert.match(source, /requestId\.current/);
  assert.doesNotMatch(
    source,
    /setInterval|setTimeout|invoke\("(?:delete|remove|clean|migrate|execute_storage_cleanup|prepare_storage_cleanup)/,
  );
  assert.match(source, /aria-live="polite"/);
});
test("Preferences diagnostics wires wallpaper references and the command is registered", () => {
  const panel = readFileSync(
    new URL("../../components/settings/PreferencesPanel.tsx", import.meta.url),
    "utf8",
  );
  const native = readFileSync(
    new URL("../../../src-tauri/src/lib.rs", import.meta.url),
    "utf8",
  );
  assert.match(
    panel,
    /<DiagnosticsPanel\s+active=\{activeCategory === "diagnostics"\}/,
  );
  assert.match(native, /commands::config::inspect_home_storage/);
});
