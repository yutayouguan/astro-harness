import assert from "node:assert/strict";
import test from "node:test";
import { LUCIDE_AGENT_ICONS } from "./lucideAgentIcons.ts";

test("every agent icon has morphable Lucide data", () => {
  assert.equal(LUCIDE_AGENT_ICONS.length, 232);
  for (const icon of LUCIDE_AGENT_ICONS) {
    assert.equal(icon.data[0], "svg", `${icon.id} should retain its SVG root`);
    assert.ok(
      Array.isArray(icon.data[2]),
      `${icon.id} should include path children`,
    );
    assert.ok(icon.data[2]?.length, `${icon.id} should include drawable paths`);
  }
});
