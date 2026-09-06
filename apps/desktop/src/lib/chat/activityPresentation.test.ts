import { test } from "node:test";
import assert from "node:assert/strict";
import type { ChatActivity } from "../../types.ts";
import {
  activityGroupProgress,
  activityGroupSummary,
  activityDisplayTarget,
  activityLinkPresentation,
  activityTitlePresentation,
  activityVisualKind,
  distinctActivityVisualKinds,
} from "./activityPresentation.ts";

function activity(
  title: string,
  kind: ChatActivity["kind"] = "tool",
): ChatActivity {
  return { id: title, kind, title };
}

test("classifies native tool names into visual verbs", () => {
  assert.equal(activityVisualKind(activity("read_file")), "read");
  assert.equal(activityVisualKind(activity("web_search")), "search");
  assert.equal(activityVisualKind(activity("code_exec")), "run");
  assert.equal(activityVisualKind(activity("apply_patch")), "edit");
  assert.equal(activityVisualKind(activity("browser_navigate")), "browse");
  assert.equal(activityVisualKind(activity("image_generate")), "image");
  assert.equal(activityVisualKind(activity("default_api:image_gen")), "image");
  assert.equal(activityVisualKind(activity("video_gen")), "video");
  assert.equal(activityVisualKind(activity("music_gen")), "music");
  assert.equal(activityVisualKind(activity("speech_gen")), "speech");
  assert.equal(activityVisualKind(activity("media.image_gen")), "image");
  assert.equal(activityVisualKind(activity("media.video_gen")), "video");
  assert.equal(activityVisualKind(activity("media.music_gen")), "music");
  assert.equal(activityVisualKind(activity("media.speech_gen")), "speech");
  assert.equal(activityVisualKind(activity("browser.snapshot")), "browse");
  assert.equal(activityVisualKind(activity("render_media")), "media");
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

test("uses browser result metadata when the read call has no input target", () => {
  const value = {
    ...activity("browser.snapshot"),
    status: "done" as const,
    input: '{"action":"read"}',
    output: JSON.stringify({
      astro_browser: true,
      url: "https://www.bilibili.com/",
      title: "哔哩哔哩 (゜-゜)つロ 干杯~-bilibili",
    }),
  };
  assert.equal(
    activityDisplayTarget(value),
    "哔哩哔哩 (゜-゜)つロ 干杯~-bilibili",
  );
  assert.deepEqual(activityLinkPresentation(value), {
    url: "https://www.bilibili.com/",
    label: "哔哩哔哩 (゜-゜)つロ 干杯~-bilibili",
  });
  assert.deepEqual(activityTitlePresentation(value), {
    key: "chat.activity.item.done.read",
    target: "哔哩哔哩 (゜-゜)つロ 干杯~-bilibili",
  });
});

test("links only http browser targets and falls back to the active tab", () => {
  const value = {
    ...activity("browser.snapshot"),
    input: "{}",
    output: JSON.stringify({
      active_tab_id: "tab-b",
      tabs: [
        { id: "tab-a", url: "https://example.com", title: "Example" },
        {
          id: "tab-b",
          url: "https://www.bilibili.com/video/BV1",
          title: "B站视频",
          active: true,
        },
      ],
    }),
  };
  assert.deepEqual(activityLinkPresentation(value), {
    url: "https://www.bilibili.com/video/BV1",
    label: "B站视频",
  });
  assert.equal(
    activityLinkPresentation({
      ...activity("browser.open"),
      input: '{"url":"javascript:alert(1)"}',
    }),
    null,
  );
  assert.equal(
    activityLinkPresentation({
      ...activity("read_file"),
      input: '{"path":"site.json"}',
      output: '{"title":"Example","url":"https://example.com"}',
    }),
    null,
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
    activityGroupSummary([activity("apply_patch"), activity("exec_command")]),
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
    { ...activity("write_file"), status: "declined" as const },
  ];
  assert.deepEqual(activityGroupProgress(activities), {
    total: 4,
    waiting: 0,
    running: 0,
    retrying: 1,
    done: 1,
    partial: 0,
    error: 1,
    declined: 1,
    interrupted: 0,
    resolved: 3,
    hasPartialOutcome: true,
  });
});

test("selects semantic title keys for all eight activity states", () => {
  const states = [
    "waiting",
    "running",
    "retrying",
    "done",
    "partial",
    "error",
    "declined",
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

test("uses concise media generation titles while retaining prompt details", () => {
  const mediaCases = [
    ["image_gen", "image"],
    ["video_gen", "video"],
    ["music_gen", "music"],
    ["speech_gen", "speech"],
  ] as const;

  for (const [title, kind] of mediaCases) {
    const value = {
      ...activity(title),
      status: "running" as const,
      input: '{"operation":"generate","prompt":"a deliberately long prompt"}',
    };
    assert.equal(activityVisualKind(value), kind);
    assert.deepEqual(activityTitlePresentation(value), {
      key: `chat.activity.action.running.${kind}`,
      target: "",
    });
  }
});
