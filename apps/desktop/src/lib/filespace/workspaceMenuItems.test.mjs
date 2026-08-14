import assert from "node:assert/strict";
import test from "node:test";
import { buildWorkspaceMenuItems } from "./workspaceMenuItems.ts";

test("blank area: new + paste only", () => {
  const items = buildWorkspaceMenuItems({
    kind: "blank",
    selectedCount: 0,
    canPaste: true,
  });
  assert.deepEqual(
    items.map((i) => i.action),
    ["newFile", "newFolder", "paste"],
  );
});

test("multi select disables rename and reveal", () => {
  const items = buildWorkspaceMenuItems({
    kind: "entries",
    selectedCount: 3,
    canPaste: false,
  });
  const rename = items.find((i) => i.action === "rename");
  const reveal = items.find((i) => i.action === "reveal");
  assert.equal(rename?.disabled, true);
  assert.equal(reveal?.disabled, true);
});

test("single select enables rename", () => {
  const items = buildWorkspaceMenuItems({
    kind: "entries",
    selectedCount: 1,
    canPaste: false,
  });
  assert.equal(items.find((i) => i.action === "rename")?.disabled, false);
});
