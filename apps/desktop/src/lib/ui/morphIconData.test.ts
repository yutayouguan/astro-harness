import assert from "node:assert/strict";
import test from "node:test";
import { Menu } from "lucide";
import { toMorphIconInput } from "./morphIconData.ts";

test("toMorphIconInput unwraps lucide SVG root data", () => {
  const result = toMorphIconInput(Menu);
  assert.equal(Array.isArray(result), true);
  assert.equal(result[0]?.[0], "line");
  assert.notEqual(result[0]?.[0], "svg");
});

test("toMorphIconInput preserves morphicons child-node input", () => {
  const input = [["path", { d: "M0 0L1 1" }]] as const;
  assert.equal(toMorphIconInput(input), input);
});
