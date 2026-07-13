import { test } from "node:test";
import assert from "node:assert/strict";
import { ALLOWED_COMPONENTS, ASTRO_CATALOG_ID } from "./types.ts";

test("catalog id is v2", () => {
  assert.equal(ASTRO_CATALOG_ID, "astro://a2ui/catalog/v2");
});

test("allowlist includes extension components", () => {
  for (const name of ["Badge", "Chip", "Metric", "Avatar", "Callout", "Spacer"]) {
    assert.equal(ALLOWED_COMPONENTS.has(name), true, name);
  }
});
