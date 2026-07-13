import { test } from "node:test";
import assert from "node:assert/strict";
import { resolveActivityIO } from "./resolveActivityIO.ts";

test("prefers explicit input/output over detail", () => {
  assert.deepEqual(
    resolveActivityIO({
      id: "1",
      kind: "tool",
      title: "x",
      input: "in",
      output: "out",
      detail: "old\n→\nold2",
    }),
    { input: "in", output: "out" },
  );
});

test("splits legacy detail on arrow separator", () => {
  assert.deepEqual(
    resolveActivityIO({
      id: "1",
      kind: "tool",
      title: "x",
      detail: '{"a":1}\n→\nok',
    }),
    { input: '{"a":1}', output: "ok" },
  );
});

test("detail without separator becomes output only", () => {
  assert.deepEqual(
    resolveActivityIO({
      id: "1",
      kind: "memory",
      title: "memory_add",
      detail: "remember this",
    }),
    { output: "remember this" },
  );
});

test("empty activity yields empty io", () => {
  assert.deepEqual(
    resolveActivityIO({ id: "1", kind: "tool", title: "x" }),
    {},
  );
});
