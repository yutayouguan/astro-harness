import assert from "node:assert/strict";
import test from "node:test";
import { normalizeBrowserUrl } from "./browserUrl.ts";

test("browser addresses default public hosts to HTTPS", () => {
  assert.equal(
    normalizeBrowserUrl("example.com/docs"),
    "https://example.com/docs",
  );
});

test("browser addresses keep local development servers on HTTP", () => {
  assert.equal(normalizeBrowserUrl("localhost:5173"), "http://localhost:5173");
  assert.equal(normalizeBrowserUrl("127.0.0.1:1420"), "http://127.0.0.1:1420");
  assert.equal(
    normalizeBrowserUrl("http://0.0.0.0:3000/app"),
    "http://127.0.0.1:3000/app",
  );
});
