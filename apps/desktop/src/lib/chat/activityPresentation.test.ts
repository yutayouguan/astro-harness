import { test } from "node:test";
import assert from "node:assert/strict";
import type { ChatActivity } from "../../types.ts";
import {
  activityDisplayTarget,
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
      ...activity("terminal"),
      input: '{"command":"npm run build\\nnpm test"}',
    }),
    "npm run build",
  );
});

test("classifies multiplexed tools from their structured operation", () => {
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
      activity("terminal"),
    ]),
    ["read", "search", "run"],
  );
});
