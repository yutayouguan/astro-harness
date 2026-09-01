import assert from "node:assert/strict";
import { test } from "node:test";
import {
  buildElicitationContent,
  elicitationRequestId,
  resolveElicitationAction,
} from "./elicitation.ts";

const interrupt = {
  id: "normalized-id",
  reason: "elicitation",
  responseSchema: {
    type: "object",
    properties: {
      region: { type: "string" },
      replicas: { type: "integer" },
      dryRun: { type: "boolean" },
    },
  },
  metadata: {
    payload: { mcp_request_id: "raw-id" },
  },
};

test("MCP elicitation returns schema-shaped and typed content", () => {
  assert.deepEqual(
    buildElicitationContent(interrupt, {
      answers: { region: "east", replicas: "3", dryRun: "true" },
      value: "summary",
    }),
    { region: "east", replicas: 3, dryRun: true },
  );
});

test("MCP elicitation uses the original request id", () => {
  assert.equal(elicitationRequestId(interrupt), "raw-id");
});

test("MCP elicitation preserves accept, decline, and cancel semantics", () => {
  assert.equal(resolveElicitationAction("choose"), "accept");
  assert.equal(resolveElicitationAction("deny"), "decline");
  assert.equal(resolveElicitationAction("cancel"), "cancel");
});
