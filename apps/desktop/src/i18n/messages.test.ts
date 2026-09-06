import assert from "node:assert/strict";
import test from "node:test";
import { translate } from "./messages.ts";

test("translate resolves double-brace diagnostics placeholders without visible braces", () => {
  assert.equal(
    translate("zh", "prefs.diag.status.backendDetail", {
      endpoint: "127.0.0.1:50051",
    }),
    "127.0.0.1:50051 · 内嵌",
  );
  assert.equal(
    translate("zh", "prefs.diag.status.databaseDetail", { version: "22" }),
    "schema v22",
  );
});

test("translate replaces repeated and legacy single-brace placeholders", () => {
  assert.equal(
    translate("en", "prefs.diag.results", { shown: "5", total: "8" }),
    "Showing 5 / 8 lines",
  );
});
