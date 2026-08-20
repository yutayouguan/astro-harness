import { test } from "node:test";
import assert from "node:assert/strict";
import { parseHitlRunFinished } from "./hitlRunFinished.ts";

test("RunFinished metadata creates a clickable surface linked to pending interrupts", () => {
  const result = parseHitlRunFinished(
    JSON.stringify([
      {
        id: "request-1",
        reason: "confirmation",
        message: "Continue?",
        tool_call_id: "tool-1",
        response_schema_json: JSON.stringify({
          type: "object",
          properties: { approved: { type: "boolean" } },
        }),
        metadata_json: JSON.stringify({
          kind: "request_user_input",
          operations: [{ op: "surfaceUpdate", path: "/approved" }],
        }),
      },
    ]),
    "assistant-1",
  );

  assert.equal(result.interrupts.length, 1);
  assert.equal(result.interrupts[0]?.id, "request-1");
  assert.equal(result.interrupts[0]?.assistantMessageId, "assistant-1");
  assert.equal(result.surface?.messageId, "a2ui-surface-tool-1");
  assert.deepEqual(result.surface?.operations, [
    { op: "surfaceUpdate", path: "/approved" },
  ]);
  assert.deepEqual(result.surface?.interrupts?.map((item) => item.id), ["request-1"]);
});

test("malformed metadata keeps pending interrupt without inventing a surface", () => {
  const result = parseHitlRunFinished(
    JSON.stringify([
      {
        id: "request-2",
        reason: "input_required",
        metadata_json: "not-json",
      },
    ]),
    "assistant-2",
  );
  assert.equal(result.interrupts[0]?.id, "request-2");
  assert.equal(result.surface, undefined);
});
