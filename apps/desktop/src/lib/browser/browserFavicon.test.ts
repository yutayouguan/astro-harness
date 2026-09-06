import assert from "node:assert/strict";
import test from "node:test";
import { normalizeBrowserFaviconUrl } from "./browserFavicon.ts";

test("browser favicons accept website and compact raster data URLs", () => {
  assert.equal(
    normalizeBrowserFaviconUrl("https://static.example.com/favicon.ico"),
    "https://static.example.com/favicon.ico",
  );
  assert.equal(
    normalizeBrowserFaviconUrl("data:image/png;base64,AA=="),
    "data:image/png;base64,AA==",
  );
});

test("browser favicons reject active, local, and oversized sources", () => {
  assert.equal(normalizeBrowserFaviconUrl("javascript:alert(1)"), null);
  assert.equal(normalizeBrowserFaviconUrl("file:///tmp/favicon.ico"), null);
  assert.equal(normalizeBrowserFaviconUrl("data:image/svg+xml,<svg/>"), null);
  assert.equal(
    normalizeBrowserFaviconUrl(`https://example.com/${"a".repeat(8_200)}`),
    null,
  );
});
