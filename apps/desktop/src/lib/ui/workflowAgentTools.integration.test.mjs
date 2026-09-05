import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const read = (path) => readFileSync(new URL(path, import.meta.url), "utf8");

const loopPanel = read("../../components/loop/LoopPanel.tsx");
const toolCatalog = read("../../hooks/providers/useAgentTools.ts");
const tauriConfig = read("../../../src-tauri/src/commands/config.rs");
const agentRuntime = read("../../../../../crates/agent-core/src/runtime/mod.rs");
const workflowRuntime = read(
  "../../../../../crates/agent-tools/src/engine/workflow.rs",
);

test("workflow agent tools are configured, catalogued, and refreshed per step", () => {
  assert.match(loopPanel, /set_loop_agent_tool/);
  assert.match(loopPanel, /input_schema/);
  assert.match(loopPanel, /confirmation/);
  assert.match(toolCatalog, /id: "workflow"/);
  assert.match(tauriConfig, /register_workflow_tools/);
  assert.match(agentRuntime, /reload_workflow_tools\(\)\?/);
});

test("workflow tools use native namespace and refuse eager fallback", () => {
  assert.match(workflowRuntime, /WORKFLOW_NAMESPACE: &str = "workflow"/);
  assert.match(workflowRuntime, /allow_eager_fallback: false/);
  assert.match(workflowRuntime, /workflow__\{\}/);
  assert.match(workflowRuntime, /validate_agent_tool_input/);
});
