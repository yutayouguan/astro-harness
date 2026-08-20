import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

const source = readFileSync(
  fileURLToPath(new URL("./useAgentTools.ts", import.meta.url)),
  "utf8",
);
const subagentFallback = source.match(
  /\{\s*id: "subagents",[\s\S]*?\n\s*\},\n\s*\{\s*id: "cron",/,
)?.[0];

test("subagent fallback exposes only the six Codex V2 model tools", () => {
  assert.ok(subagentFallback, "subagent fallback block must exist");
  const expected = [
    "spawn_agent",
    "list_agents",
    "send_message",
    "followup_task",
    "wait_agent",
    "interrupt_agent",
  ];
  for (const name of expected) {
    assert.match(subagentFallback, new RegExp(`name: "${name}"`));
  }
  for (const legacy of [
    'name: "task"',
    'name: "agent_name"',
    'name: "read_agent"',
    'name: "close_agent"',
    'name: "send_message_to_agent"',
    'name: "wait_agents"',
  ]) {
    assert.equal(subagentFallback.includes(legacy), false, legacy);
  }
});
