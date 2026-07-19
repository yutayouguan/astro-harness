import assert from "node:assert/strict";
import test from "node:test";
import {
  HEADER_AGENT_PICKER_NAV_IDS,
  showsHeaderAgentPicker,
} from "./headerAgentPicker.ts";

test("showsHeaderAgentPicker covers agent-scoped tabs only", () => {
  for (const id of HEADER_AGENT_PICKER_NAV_IDS) {
    assert.equal(showsHeaderAgentPicker(id), true, id);
  }
  for (const id of ["chat", "memory", "providers", "settings"] as const) {
    assert.equal(showsHeaderAgentPicker(id), false, id);
  }
});
