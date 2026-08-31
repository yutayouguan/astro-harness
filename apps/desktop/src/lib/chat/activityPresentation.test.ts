import { test } from "node:test";
import assert from "node:assert/strict";
import type { ChatActivity } from "../../types.ts";
import {
  activityGroupProgress,
  activityGroupSummary,
  activityDisplayTarget,
  activityTitlePresentation,
  activityVisualKind,
  distinctActivityVisualKinds,
} from "./activityPresentation.ts";

function activity(title: string, kind: ChatActivity["kind"] = "tool"): ChatActivity {
  return { id: title, kind, title };
}

test("classifies native tool names into visual verbs", () => {
  assert.equal(activityVisualKind(activity("read_file")), "read");
  assert.equal(activityVisualKind(activity("web_search")), "search");
  assert.equal(activityVisualKind(activity("code_exec")), "run");
  assert.equal(activityVisualKind(activity("apply_patch")), "edit");
  assert.equal(activityVisualKind(activity("browser_navigate")), "browse");
  assert.equal(activityVisualKind(activity("image_generate")), "media");
  assert.equal(activityVisualKind(activity("custom_tool")), "tool");
});

test("extracts compact targets for human-readable rows", () => {
  assert.equal(
    activityDisplayTarget({
      ...activity("read_file"),
      input: '{"path":"apps/desktop/src/ChatView.tsx"}',
    }),
    "ChatView.tsx",
  );
  assert.equal(
    activityDisplayTarget({
      ...activity("web_search"),
      input: '{"query":"Responses API events"}',
    }),
    "Responses API events",
  );
  assert.equal(
    activityDisplayTarget({
      ...activity("exec_command"),
      input: '{"command":"npm run build\\nnpm test"}',
    }),
    "npm run build",
  );
  assert.equal(
    activityDisplayTarget({
      ...activity("apply_patch"),
      input:
        '"*** Begin Patch\\n*** Update File: apps/desktop/src/components/chat/ChatView.tsx\\n@@\\n*** End Patch"',
    }),
    "ChatView.tsx",
  );
  assert.equal(
    activityDisplayTarget({
      ...activity("apply_patch"),
      input:
        "*** Begin Patch\n*** Update File: apps/desktop/src/ChatView.tsx\n*** Add File: apps/desktop/src/NewPanel.tsx\n*** End Patch",
    }),
    "ChatView.tsx +1",
  );
  assert.equal(
    activityDisplayTarget({
      ...activity("apply_patch"),
      input:
        '{"patch":"*** Begin Patch\\n*** Update File: apps/desktop/src/MsgActivity.tsx\\n*** End Patch"}',
    }),
    "MsgActivity.tsx",
  );
});

test("classifies multiplexed tools from their structured operation", () => {
  assert.equal(
    activityVisualKind({
      ...activity("exec_command"),
      input: '{"operation":"read","path":"README.md"}',
    }),
    "read",
  );
  assert.equal(
    activityVisualKind({
      ...activity("skills"),
      input: '{"action":"search","query":"timeline"}',
    }),
    "search",
  );
  assert.equal(
    activityVisualKind({
      ...activity("apply_patch"),
      input: '{"path":"ChatView.tsx"}',
    }),
    "edit",
  );
});

test("preserves non-tool activity kinds", () => {
  assert.equal(activityVisualKind(activity("load", "skill")), "skill");
  assert.equal(activityVisualKind(activity("server.call", "mcp")), "mcp");
  assert.equal(activityVisualKind(activity("remember", "memory")), "memory");
});

test("returns distinct visual kinds in event order", () => {
  assert.deepEqual(
    distinctActivityVisualKinds([
      activity("read_file"),
      activity("read_file"),
      activity("rg_search"),
      activity("exec_command"),
    ]),
    ["read", "search", "run"],
  );
});

test("summarizes the batch intent instead of exposing a raw verb list", () => {
  assert.equal(
    activityGroupSummary([
      activity("apply_patch"),
      activity("exec_command"),
    ]),
    "modify_and_verify",
  );
  assert.equal(
    activityGroupSummary([activity("web_search"), activity("read_file")]),
    "research",
  );
});

test("summarizes live and partial group progress", () => {
  const activities = [
    { ...activity("read_file"), status: "done" as const },
    { ...activity("exec_command"), status: "retrying" as const },
    { ...activity("apply_patch"), status: "error" as const },
  ];
  assert.deepEqual(activityGroupProgress(activities), {
    total: 3,
    waiting: 0,
    running: 0,
    retrying: 1,
    done: 1,
    partial: 0,
    error: 1,
    interrupted: 0,
    resolved: 2,
    hasPartialOutcome: true,
  });
});

test("selects semantic title keys for all seven activity states", () => {
  const states = [
    "waiting",
    "running",
    "retrying",
    "done",
    "partial",
    "error",
    "interrupted",
  ] as const;
  assert.deepEqual(
    states.map((status) =>
      activityTitlePresentation({
        ...activity("exec_command"),
        input: '{"cmd":"npm test"}',
        status,
      }),
    ),
    states.map((status) => ({
      key: `chat.activity.item.${status}.run`,
      target: "npm test",
    })),
  );
  assert.equal(activityTitlePresentation(activity("custom_tool")), null);
});
