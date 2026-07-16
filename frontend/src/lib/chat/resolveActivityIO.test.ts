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
      detail: "ignored",
    }),
    { input: "in", output: "out" },
  );
});

test("detail without input/output becomes output only", () => {
  assert.deepEqual(
    resolveActivityIO({
      id: "1",
      kind: "memory",
      title: "memory",
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
