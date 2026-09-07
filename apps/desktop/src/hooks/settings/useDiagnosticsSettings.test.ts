import assert from "node:assert/strict";
import test from "node:test";

import {
  buildDiagnosticStatusCards,
  diagnosticLogLevel,
  type DiagnosticsStatusDto,
} from "../../lib/settings/diagnosticsModel.ts";

const t = (key: string, vars?: Record<string, string>) =>
  vars ? `${key}:${JSON.stringify(vars)}` : key;

const healthyStatus: DiagnosticsStatusDto = {
  backendHealthy: true,
  backendEndpoint: "http://127.0.0.1:4311",
  backendError: null,
  providerEnabled: 2,
  providerTotal: 2,
  activeProviderId: "openai",
  activeProviderName: "OpenAI",
  providerError: null,
  mcpConnected: 1,
  mcpTotal: 1,
  mcpRetrying: 0,
  mcpError: null,
  databaseHealthy: true,
  databaseJournalMode: "wal",
  databaseSchemaVersion: 22,
  databaseError: null,
};

test("diagnostic log severity recognizes backend log variants", () => {
  assert.equal(diagnosticLogLevel("critical failure"), "error");
  assert.equal(diagnosticLogLevel("WARN reconnecting"), "warn");
  assert.equal(diagnosticLogLevel("info ready"), "info");
  assert.equal(diagnosticLogLevel("trace request"), "debug");
  assert.equal(diagnosticLogLevel("plain output"), "unknown");
});

test("diagnostic status projection keeps health semantics out of the panel", () => {
  const healthy = buildDiagnosticStatusCards(healthyStatus, false, "", t);
  assert.deepEqual(
    healthy.map((card) => [card.id, card.state]),
    [
      ["backend", "healthy"],
      ["provider", "healthy"],
      ["mcp", "healthy"],
      ["database", "healthy"],
    ],
  );

  const degraded = buildDiagnosticStatusCards(
    {
      ...healthyStatus,
      providerError: "offline",
      mcpTotal: 0,
      mcpConnected: 0,
      databaseHealthy: false,
    },
    false,
    "",
    t,
  );
  assert.deepEqual(
    degraded.map((card) => [card.id, card.state]),
    [
      ["backend", "healthy"],
      ["provider", "error"],
      ["mcp", "unknown"],
      ["database", "error"],
    ],
  );
});

test("diagnostic status projection exposes loading and unavailable states", () => {
  const loading = buildDiagnosticStatusCards(null, true, "", t);
  assert.ok(loading.every((card) => card.state === "unknown"));
  assert.ok(
    loading.every((card) => card.value === "prefs.diag.status.checking"),
  );

  const failed = buildDiagnosticStatusCards(null, false, "offline", t);
  assert.ok(
    failed.every((card) => card.detail === "prefs.diag.status.unavailable"),
  );
});
