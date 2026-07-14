import assert from "node:assert/strict";
import { test } from "node:test";
import { elapsedSecSince, formatElapsedSec } from "./elapsedSec.ts";

test("formatElapsedSec keeps one decimal under 10s", () => {
  assert.equal(formatElapsedSec(0.1), "0.1");
  assert.equal(formatElapsedSec(3.24), "3.2");
  assert.equal(formatElapsedSec(9.94), "9.9");
});

test("formatElapsedSec rounds at and above 10s", () => {
  assert.equal(formatElapsedSec(10), "10");
  assert.equal(formatElapsedSec(12.4), "12");
  assert.equal(formatElapsedSec(12.6), "13");
});

test("elapsedSecSince floors to 0.1", () => {
  assert.equal(elapsedSecSince(1000, 1000), 0.1);
  assert.equal(elapsedSecSince(1000, 1500), 0.5);
  assert.equal(elapsedSecSince(1000, 11200), 10.2);
});
