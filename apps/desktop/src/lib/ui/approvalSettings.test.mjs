import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const root = new URL("../../", import.meta.url);

test("approval settings use the canonical three permission presets", async () => {
  const source = await readFile(new URL("components/settings/ToolsPanel.tsx", root), "utf8");

  assert.match(source, /"ask_for_approval", "approve_for_me", "full_access"/);
  assert.match(source, /invoke\("set_permission_preset", \{ preset, confirmed \}\)/);
  assert.doesNotMatch(source, /invoke<ApprovalSettings>\("set_approval_mode"/);
});

test("approval UI and resume payload expose scoped command-type permission", async () => {
  const wizard = await readFile(new URL("a2ui/ClarifyWizard.tsx", root), "utf8");
  const session = await readFile(new URL("hooks/chat/useChatSession.ts", root), "utf8");

  assert.match(wizard, /approvalTypeLabel/);
  assert.match(wizard, /submitApproval\("approve_type"\)/);
  assert.match(session, /scope: "type"/);
});
