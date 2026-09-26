import assert from "node:assert/strict";
import test from "node:test";
import { en } from "./catalogs/en.ts";
import { zh } from "./catalogs/zh.ts";
import { translate } from "./messages.ts";

test("translate resolves diagnostics placeholders without visible braces", () => {
  assert.equal(
    translate(zh, "prefs.diag.status.backendDetail", {
      endpoint: "127.0.0.1:50051",
    }),
    "127.0.0.1:50051 · 内嵌",
  );
  assert.equal(
    translate(zh, "prefs.diag.status.databaseDetail", { version: "22" }),
    "schema v22",
  );
});

test("translate replaces every canonical placeholder", () => {
  assert.equal(
    translate(en, "prefs.diag.results", { shown: "5", total: "8" }),
    "Showing 5 / 8 lines",
  );
});

test("translatable placeholders use the single-brace contract", () => {
  for (const catalog of [zh, en]) {
    for (const [key, value] of Object.entries(catalog)) {
      if (key === "loop.variablesHint") continue;
      assert.doesNotMatch(value, /\{\{[A-Za-z]/, key);
    }
  }
});
