import assert from "node:assert/strict";
import test from "node:test";
import { elapsedSecSince, formatElapsedSec } from "./elapsedSec.ts";

test("formatElapsedSec uses ms under 1s", () => {
  assert.equal(formatElapsedSec(0), "0ms");
  assert.equal(formatElapsedSec(0.012), "12ms");
  assert.equal(formatElapsedSec(0.1), "100ms");
  assert.equal(formatElapsedSec(0.999), "999ms");
});

test("formatElapsedSec keeps one decimal under 10s", () => {
  assert.equal(formatElapsedSec(1), "1.0s");
  assert.equal(formatElapsedSec(3.24), "3.2s");
  assert.equal(formatElapsedSec(9.94), "9.9s");
});

test("formatElapsedSec rounds at and above 10s", () => {
  assert.equal(formatElapsedSec(10), "10s");
  assert.equal(formatElapsedSec(12.4), "12s");
  assert.equal(formatElapsedSec(12.6), "13s");
});

test("elapsedSecSince has ms precision and no floor", () => {
  assert.equal(elapsedSecSince(1000, 1000), 0);
  assert.equal(elapsedSecSince(1000, 1012), 0.012);
  assert.equal(elapsedSecSince(1000, 1500), 0.5);
  assert.equal(elapsedSecSince(1000, 11200), 10.2);
});
