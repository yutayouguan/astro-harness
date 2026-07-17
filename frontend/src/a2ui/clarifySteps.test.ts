import { test } from "node:test";
import assert from "node:assert/strict";
import { parseClarifySteps } from "./clarifySteps.ts";

test("parseClarifySteps normalizes ids and options", () => {
  const steps = parseClarifySteps([
    { question: "风格？", options: ["民谣", "电子"] },
    { id: "lyrics", question: "歌词？", options: [{ label: "你写", value: "write" }] },
    { question: "  ", options: ["x"] },
  ]);
  assert.equal(steps.length, 2);
  assert.equal(steps[0].id, "q0");
  assert.deepEqual(steps[0].options, ["民谣", "电子"]);
  assert.equal(steps[1].id, "lyrics");
  assert.deepEqual(steps[1].options, ["write"]);
});

test("parseClarifySteps empty options get fallback", () => {
  const steps = parseClarifySteps([{ id: "a", question: "继续？", options: [] }]);
  assert.equal(steps.length, 1);
  assert.deepEqual(steps[0].options, ["继续"]);
});
