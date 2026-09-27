import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const root = new URL("../../", import.meta.url);

test("approval settings use the canonical three permission presets", async () => {
  const source = await readFile(
    new URL("components/settings/ToolsPanel.tsx", root),
    "utf8",
  );

  assert.match(
    source,
    /"ask_for_approval",\s*"approve_for_me",\s*"full_access"/,
  );
  assert.match(
    source,
    /invoke\("set_permission_preset", \{ preset, confirmed \}\)/,
  );
  assert.doesNotMatch(source, /invoke<ApprovalSettings>\("set_approval_mode"/);
});

test("approval UI and resume payload expose scoped command-type permission", async () => {
  const wizard = await readFile(
    new URL("a2ui/ClarifyWizard.tsx", root),
    "utf8",
  );
  const session = await readFile(
    new URL("hooks/chat/useChatSession.ts", root),
    "utf8",
  );

  assert.match(wizard, /approvalTypeLabel/);
  // 动作 id 由 approvalKind 映射到同一套 submitApproval：确认/沙箱用 approve*，
  // 网络授权用 allow*（会话级）。
  assert.match(wizard, /id: "approve_type" as const/);
  assert.match(wizard, /id: "allow_session" as const/);
  assert.match(wizard, /submitApproval\((?:actions\.\w+\.id|action\.id)\)/);
  assert.match(session, /scope: "type"/);
  assert.match(session, /scope: "allow_session"/);
});

test("command approval separates request, command, actions, and composer surfaces", async () => {
  const [wizard, activityStyles, composerStyles] = await Promise.all([
    readFile(new URL("a2ui/ClarifyWizard.tsx", root), "utf8"),
    readFile(new URL("styles/features/chat/activity.css", root), "utf8"),
    readFile(new URL("styles/features/chat/markdown.css", root), "utf8"),
  ]);

  assert.match(wizard, /className="a2ui-approval-intro"/);
  assert.match(wizard, /className="a2ui-approval-command"/);
  assert.match(wizard, /className="a2ui-approval-footer"/);
  assert.match(activityStyles, /\.a2ui-approval-persistent-actions\s*\{/);
  assert.match(
    composerStyles,
    /\.a2ui-clarify-wizard:is\(\.is-approval, \.is-approval-result\)/,
  );
  assert.match(
    composerStyles,
    /\.composer--stacked\.has-clarify:has\([\s\S]*?border-color:\s*transparent;[\s\S]*?background:\s*transparent;[\s\S]*?box-shadow:\s*none;/,
  );
});
