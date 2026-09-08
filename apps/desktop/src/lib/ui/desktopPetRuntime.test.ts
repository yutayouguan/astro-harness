import assert from "node:assert/strict";
import test from "node:test";
import {
  applyPetActivity,
  applyPetSessionStatus,
  emptyPetRuntime,
  resolvePetRuntime,
} from "./desktopPetRuntime.ts";

test("an older snapshot cannot cut short a newer live completion", () => {
  let runtime = applyPetActivity(
    emptyPetRuntime(),
    { sessionId: "a", state: "jumping", tsMs: 20 },
    0,
  );
  runtime = applyPetSessionStatus(runtime, {
    sessionId: "a",
    status: "idle",
    tsMs: 10,
  });
  assert.equal(resolvePetRuntime(runtime, 100), "jumping");
});

test("replayed and stale activity cannot revive a completed or superseded turn", () => {
  let runtime = applyPetSessionStatus(emptyPetRuntime(), {
    sessionId: "a",
    status: "active",
    tsMs: 100,
  });
  runtime = applyPetActivity(
    runtime,
    { sessionId: "a", state: "review", tsMs: 101 },
    0,
  );
  assert.equal(resolvePetRuntime(runtime, 1), "review");
  runtime = applyPetSessionStatus(runtime, {
    sessionId: "a",
    status: "idle",
    tsMs: 102,
  });
  runtime = applyPetActivity(
    runtime,
    { sessionId: "a", state: "review", tsMs: 101 },
    2,
  );
  assert.equal(resolvePetRuntime(runtime, 3), "idle");
  runtime = applyPetSessionStatus(runtime, {
    sessionId: "a",
    status: "active",
    tsMs: 200,
  });
  runtime = applyPetActivity(
    runtime,
    { sessionId: "a", state: "jumping", tsMs: 103 },
    4,
  );
  assert.equal(resolvePetRuntime(runtime, 5), "running");
});

test("an unrelated task cannot clear waiting and transient expiry restores aggregate state", () => {
  let runtime = applyPetSessionStatus(emptyPetRuntime(), {
    sessionId: "a",
    status: "active",
    activeFlags: ["waitingOnApproval"],
    tsMs: 1,
  });
  runtime = applyPetActivity(
    runtime,
    { sessionId: "b", state: "failed", tsMs: 2 },
    0,
  );
  assert.equal(resolvePetRuntime(runtime, 1), "waiting");
  runtime = applyPetSessionStatus(runtime, {
    sessionId: "a",
    status: "active",
    tsMs: 3,
  });
  assert.equal(resolvePetRuntime(runtime, 2201), "running");
});

test("idle and interruption do not imply success, and duplicate completion does not restart animation", () => {
  let runtime = applyPetSessionStatus(emptyPetRuntime(), {
    sessionId: "a",
    status: "idle",
    tsMs: 10,
  });
  assert.equal(resolvePetRuntime(runtime, 0), "idle");
  runtime = applyPetActivity(
    runtime,
    { sessionId: "a", state: "jumping", tsMs: 11 },
    0,
  );
  runtime = applyPetActivity(
    runtime,
    { sessionId: "a", state: "jumping", tsMs: 11 },
    900,
  );
  assert.equal(resolvePetRuntime(runtime, 981), "idle");
});
