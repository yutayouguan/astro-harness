import assert from "node:assert/strict";
import { test } from "node:test";
import { parseUnifiedDiff } from "./unifiedDiff.ts";

test("tracks old and new line numbers across a unified diff hunk", () => {
  const rows = parseUnifiedDiff(
    "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -4,3 +4,4 @@\n same\n-old\n+new\n+extra\n tail\n",
  );

  assert.deepEqual(
    rows.filter((row) => ["context", "addition", "deletion"].includes(row.kind)),
    [
      { kind: "context", content: "same", oldLine: 4, newLine: 4 },
      { kind: "deletion", content: "old", oldLine: 5, newLine: null },
      { kind: "addition", content: "new", oldLine: null, newLine: 5 },
      { kind: "addition", content: "extra", oldLine: null, newLine: 6 },
      { kind: "context", content: "tail", oldLine: 6, newLine: 7 },
    ],
  );
});

test("keeps patch headers as metadata instead of counting them as changes", () => {
  const rows = parseUnifiedDiff("--- /dev/null\n+++ b/new.ts\n@@ -0,0 +1 @@\n+hello\n");
  assert.equal(rows.filter((row) => row.kind === "meta").length, 2);
  assert.deepEqual(rows.at(-1), {
    kind: "addition",
    content: "hello",
    oldLine: null,
    newLine: 1,
  });
});
