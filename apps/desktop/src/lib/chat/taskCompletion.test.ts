import assert from "node:assert/strict";
import { test } from "node:test";
import {
  resolveParallelTaskCompletion,
  resolveTaskCompletion,
  shouldStartCompletionCelebration,
} from "./taskCompletion.ts";

const emptyResponseError = "empty response";

test("only an explicit successful task with visible output celebrates", () => {
  assert.deepEqual(
    resolveTaskCompletion({
      outcome: "success",
      hasRenderableOutput: true,
      emptyResponseError,
    }),
    { failed: false, error: null, celebrate: true },
  );
  assert.deepEqual(
    resolveTaskCompletion({
      outcome: "success",
      hasRenderableOutput: false,
      emptyResponseError,
    }),
    { failed: true, error: emptyResponseError, celebrate: false },
  );
  assert.equal(
    resolveTaskCompletion({
      outcome: "success",
      terminalError: "stream failed",
      hasRenderableOutput: true,
      emptyResponseError,
    }).celebrate,
    false,
  );
  assert.equal(
    resolveTaskCompletion({
      outcome: null,
      hasRenderableOutput: true,
      emptyResponseError,
    }).celebrate,
    false,
  );
});

test("parallel completion preserves interrupt and HITL terminal semantics", () => {
  assert.deepEqual(
    resolveParallelTaskCompletion({
      outcome: "interrupt",
      hasRenderableOutput: false,
      emptyResponseError,
    }),
    { status: "cancelled", failed: false, error: null, celebrate: false },
  );
  assert.deepEqual(
    resolveParallelTaskCompletion({
      outcome: "hitl_waiting",
      hasRenderableOutput: true,
      emptyResponseError,
    }),
    { status: null, failed: false, error: null, celebrate: false },
  );
  assert.equal(
    resolveParallelTaskCompletion({
      outcome: "error",
      terminalError: "provider failed",
      hasRenderableOutput: true,
      emptyResponseError,
    }).status,
    "error",
  );
  assert.deepEqual(
    resolveParallelTaskCompletion({
      outcome: null,
      hasRenderableOutput: true,
      emptyResponseError,
    }),
    { status: "done", failed: false, error: null, celebrate: false },
  );
});

test("celebration starts only for a strictly newer trigger", () => {
  assert.equal(shouldStartCompletionCelebration(0, 1), true);
  assert.equal(shouldStartCompletionCelebration(4, 5), true);
  assert.equal(shouldStartCompletionCelebration(4, 4), false);
  assert.equal(shouldStartCompletionCelebration(4, 0), false);
  assert.equal(shouldStartCompletionCelebration(4, 3), false);
  assert.equal(shouldStartCompletionCelebration(7, 7), false);
});
