import { sameDiagnosticLog } from "./logView.ts";
import assert from "node:assert/strict";
import test from "node:test";
import {
  diagnosticLogTimeBounds,
  presentDiagnosticMessage,
  toLocalDateTimeInput,
} from "./logView.ts";

test("rolling diagnostics ranges move with the current time", () => {
  assert.deepEqual(diagnosticLogTimeBounds("15m", "", "", 1_000_000), {
    sinceMs: 100_000,
    untilMs: null,
  });
  assert.deepEqual(diagnosticLogTimeBounds("all", "", "", 1_000_000), {
    sinceMs: null,
    untilMs: null,
  });
});

test("custom diagnostics range parses local datetime inputs", () => {
  const start = toLocalDateTimeInput(Date.now() - 60_000);
  const end = toLocalDateTimeInput(Date.now());
  const bounds = diagnosticLogTimeBounds("custom", start, end);
  assert.ok(bounds.sinceMs != null);
  assert.ok(bounds.untilMs != null);
  assert.ok(bounds.sinceMs <= bounds.untilMs);
});

test("diagnostic messages render escaped line breaks as readable text", () => {
  assert.equal(
    presentDiagnosticMessage("query=one\\nnext=two\\tvalue"),
    "query=one\nnext=two  value",
  );
});

test("selected log identity survives fresh row objects but not changed events", () => {
  const row = {
    source: "agent",
    timestamp: "2026-09-14T02:00:00Z",
    level: "INFO",
    message: "Ready",
    raw: "INFO Ready",
  };
  assert.equal(sameDiagnosticLog(row, { ...row }), true);
  assert.equal(sameDiagnosticLog(null, row), false);
  assert.equal(sameDiagnosticLog(row, { ...row, raw: "INFO Updated" }), false);
  assert.equal(sameDiagnosticLog(row, { ...row, source: "errors" }), false);
  assert.equal(
    sameDiagnosticLog(row, { ...row, timestamp: "2026-09-14T02:00:01Z" }),
    false,
  );
});
